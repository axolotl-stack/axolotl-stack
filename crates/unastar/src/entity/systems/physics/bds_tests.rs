//! Native BDS 1.26.51 trajectories and liquid/cadence boundary observations.

use super::tests::TestBlocks;
use super::*;
use crate::registry::block::StateValue;
use serde_json::Value;
use std::sync::OnceLock;

/// The disposable world's recorded geometry in the v5 BDS probe.
struct BdsMotionWorld {
    stone: u32,
    water: u32,
    lava: u32,
    slab: u32,
}

impl BdsMotionWorld {
    fn new() -> Self {
        let registry = BlockRegistry::vanilla();
        let slab = registry.get_by_name("minecraft:oak_slab").unwrap();
        Self {
            stone: registry
                .default_state_id_by_name("minecraft:stone")
                .unwrap(),
            water: registry
                .default_state_id_by_name("minecraft:water")
                .unwrap(),
            lava: registry.default_state_id_by_name("minecraft:lava").unwrap(),
            slab: slab
                .with_state_value(
                    slab.default_state_id,
                    "minecraft:vertical_half",
                    crate::registry::block::StateValue::Str("bottom"),
                )
                .unwrap(),
        }
    }
}

impl BlockSource for BdsMotionWorld {
    fn block_at(&self, pos: IVec3) -> Option<u32> {
        let IVec3 { x, y, z } = pos;
        let main_floor = (-25..=25).contains(&x) && (-25..=25).contains(&z);
        let fluid_columns = (24..=40).contains(&z);
        let water_column = (24..=40).contains(&x) && fluid_columns;
        let lava_column = (-40..=-24).contains(&x) && fluid_columns;
        Some(if y == -61 && (main_floor || water_column || lava_column) {
            self.stone
        } else if (-60..=-20).contains(&y) && water_column {
            self.water
        } else if (-60..=-20).contains(&y) && lava_column {
            self.lava
        } else if y == -60 && (16..=20).contains(&x) && (-2..=2).contains(&z) {
            self.slab
        } else {
            *crate::world::chunk::blocks::AIR
        })
    }

    fn registry(&self) -> &BlockRegistry {
        BlockRegistry::vanilla()
    }
}

fn vector(row: &Value, key: &str) -> DVec3 {
    DVec3::new(
        row[key][0].as_f64().unwrap(),
        row[key][1].as_f64().unwrap(),
        row[key][2].as_f64().unwrap(),
    )
}

fn assert_native_step(
    blocks: &impl BlockSource,
    body: &PhysicsBody,
    before: &Value,
    after: &Value,
    age: u64,
    case: &str,
) {
    let tick = before["tick"].as_u64().unwrap();
    assert_eq!(after["tick"].as_u64().unwrap(), tick + 1);
    let mut velocity = vector(before, "velocity");
    let mut grounded = before["on_ground"].as_bool().unwrap();
    let position = step_body(
        blocks,
        body,
        vector(before, "position"),
        &mut velocity,
        &mut grounded,
        age,
    );
    // Compare one update from each native snapshot, not accumulated f64
    // simulation error against BDS's f32 coordinate storage.
    assert!(
        (position - vector(after, "position")).abs().max_element() < 0.00002,
        "{case} tick {tick}: position {position:?}, BDS {}",
        after["position"]
    );
    assert!(
        (velocity - vector(after, "velocity")).abs().max_element() < 0.000001,
        "{case} tick {tick}: velocity {velocity:?}, BDS {}",
        after["velocity"]
    );
    assert_eq!(
        grounded,
        after["on_ground"].as_bool().unwrap(),
        "{case} tick {tick}: grounded"
    );
}

fn assert_trajectory(
    blocks: &impl BlockSource,
    body: &PhysicsBody,
    rows: &[Value],
    spawn_tick: u64,
    case: &str,
) {
    for pair in rows.windows(2) {
        let (before, after) = (&pair[0], &pair[1]);
        if before["valid"] == true && after["valid"] == true {
            assert_native_step(
                blocks,
                body,
                before,
                after,
                before["tick"].as_u64().unwrap() - spawn_tick,
                case,
            );
        }
    }
}

fn assert_bds_motion(cases: &[(&str, &PhysicsBody)]) {
    static CAPTURE: OnceLock<Value> = OnceLock::new();
    let capture = CAPTURE.get_or_init(|| {
        serde_json::from_str(include_str!("../testdata/bds-1.26.51-motion.json")).unwrap()
    });
    let blocks = BdsMotionWorld::new();
    for (case, body) in cases {
        assert_trajectory(
            &blocks,
            body,
            capture["cases"][case].as_array().unwrap(),
            51,
            case,
        );
    }
}

fn contact_capture() -> &'static Value {
    static CAPTURE: OnceLock<Value> = OnceLock::new();
    CAPTURE.get_or_init(|| {
        serde_json::from_str(include_str!("../testdata/bds-1.26.51-contact.json")).unwrap()
    })
}

fn kinematics_capture() -> &'static Value {
    static CAPTURE: OnceLock<Value> = OnceLock::new();
    CAPTURE.get_or_init(|| {
        serde_json::from_str(include_str!("../testdata/bds-1.26.51-kinematics.json")).unwrap()
    })
}

struct ContactWorld {
    stone: u32,
    ice: u32,
    slab: u32,
    removed_support: Option<IVec3>,
}

impl ContactWorld {
    fn new(removed_support: Option<IVec3>) -> Self {
        let registry = BlockRegistry::vanilla();
        let slab = registry.get_by_name("minecraft:oak_slab").unwrap();
        Self {
            stone: registry
                .default_state_id_by_name("minecraft:stone")
                .unwrap(),
            ice: registry.default_state_id_by_name("minecraft:ice").unwrap(),
            slab: slab
                .with_state_value(
                    slab.default_state_id,
                    "minecraft:vertical_half",
                    StateValue::Str("bottom"),
                )
                .unwrap(),
            removed_support,
        }
    }
}

impl BlockSource for ContactWorld {
    fn block_at(&self, pos: IVec3) -> Option<u32> {
        let removed = self.removed_support.is_some_and(|hole| {
            pos.y == -61 && (pos.x - hole.x).abs() <= 1 && (pos.z - hole.z).abs() <= 1
        });
        Some(if removed {
            *crate::world::chunk::blocks::AIR
        } else if pos.y == -61 && (-22..=22).contains(&pos.x) && (8..=32).contains(&pos.z) {
            self.ice
        } else if (MIN_Y..=-61).contains(&pos.y) {
            // The probe's flat grass/dirt floor is collision-equivalent to
            // stone here: full cubes, native friction 0.6.
            self.stone
        } else if pos.y == -58 && (-22..=-18).contains(&pos.x) && (-2..=2).contains(&pos.z) {
            self.slab
        } else {
            *crate::world::chunk::blocks::AIR
        })
    }

    fn registry(&self) -> &BlockRegistry {
        BlockRegistry::vanilla()
    }
}

fn captured_blocks(cells: &Value) -> TestBlocks {
    let mut blocks = TestBlocks::default();
    let registry = BlockRegistry::vanilla();
    for cell in cells.as_array().unwrap() {
        let entry = registry.get_by_name(cell["id"].as_str().unwrap()).unwrap();
        let mut state = entry.default_state_id;
        for (name, value) in cell["states"].as_object().unwrap() {
            let property = entry
                .data
                .properties
                .iter()
                .find(|p| p.name == name)
                .unwrap();
            let native = property
                .values
                .iter()
                .copied()
                .find(|candidate| match candidate {
                    StateValue::Int(v) => value.as_i64() == Some(i64::from(*v)),
                    StateValue::Byte(v) => {
                        value.as_u64() == Some(u64::from(*v)) || value.as_bool() == Some(*v != 0)
                    }
                    StateValue::Str(v) => value.as_str() == Some(*v),
                })
                .unwrap();
            state = entry.with_state_value(state, name, native).unwrap();
        }
        blocks.set_state(vector(cell, "position").as_ivec3(), state);
    }
    blocks
}

#[test]
fn bds_liquid_state_boundaries_match() {
    let registry = BlockRegistry::vanilla();
    for sample in contact_capture()["fluid_steps"].as_array().unwrap() {
        let mut blocks = TestBlocks::default();
        let before = &sample["before"];
        let p = vector(before, "position").floor().as_ivec3();
        for dx in -1..=1 {
            for dz in -1..=1 {
                blocks.set(IVec3::new(p.x + dx, -61, p.z + dz), "minecraft:stone");
                if dx != 0 || dz != 0 {
                    blocks.set(IVec3::new(p.x + dx, -60, p.z + dz), "minecraft:stone");
                }
            }
        }
        let name = format!("minecraft:{}", sample["fluid"].as_str().unwrap());
        let entry = registry.get_by_name(&name).unwrap();
        let state = entry
            .with_state_value(
                entry.default_state_id,
                "liquid_depth",
                StateValue::Int(sample["depth"].as_i64().unwrap() as i32),
            )
            .unwrap();
        blocks.set_state(IVec3::new(p.x, -60, p.z), state);
        assert_native_step(
            &blocks,
            &PhysicsBody::ITEM,
            before,
            &sample["after"],
            0,
            &name,
        );
    }
}

#[test]
fn bds_stacked_liquid_and_current_cases_match() {
    for sample in kinematics_capture()["fluid_steps"].as_array().unwrap() {
        // Scheduled native block updates precede actor motion. Keep both
        // observations in the fixture, but replay the resulting block state.
        let blocks = captured_blocks(&sample["blocks"]);
        assert_native_step(
            &blocks,
            &PhysicsBody::ITEM,
            &sample["before"],
            &sample["after"],
            0,
            sample["case"].as_str().unwrap(),
        );
    }
}

#[test]
fn bds_falling_impulse_trajectory_matches() {
    let rows = contact_capture()["cases"]["falling_sand_impulse"]["samples"]
        .as_array()
        .unwrap();
    assert_trajectory(
        &ContactWorld::new(None),
        &PhysicsBody::FALLING_BLOCK,
        rows,
        0,
        "falling impulse",
    );
}

#[test]
fn bds_floating_slab_and_ice_throttle_match() {
    let blocks = ContactWorld::new(None);
    for name in ["item_floating_bottom_slab", "sleep_ice_2"] {
        let case = &contact_capture()["cases"][name];
        assert_trajectory(
            &blocks,
            &PhysicsBody::ITEM,
            case["samples"].as_array().unwrap(),
            case["spawn_tick"].as_u64().unwrap(),
            name,
        );
    }
}

#[test]
fn bds_actor_age_phases_and_support_removal_match() {
    for (name, case) in kinematics_capture()["cases"].as_object().unwrap() {
        let rows = case["samples"].as_array().unwrap();
        let hole = name
            .starts_with("wake_")
            .then(|| vector(&rows[0], "position").floor().as_ivec3());
        assert_trajectory(
            &ContactWorld::new(hole),
            &PhysicsBody::ITEM,
            rows,
            case["spawn_tick"].as_u64().unwrap(),
            name,
        );
    }
}

#[test]
fn bds_item_liquid_trajectories_match() {
    assert_bds_motion(&[
        ("item_water", &PhysicsBody::ITEM),
        ("item_lava", &PhysicsBody::ITEM),
        ("item_water_entry", &PhysicsBody::ITEM),
        ("item_lava_entry", &PhysicsBody::ITEM),
    ]);
}

#[test]
fn bds_nonliquid_actor_trajectories_match() {
    assert_bds_motion(&[
        ("item_air", &PhysicsBody::ITEM),
        ("item_bottom_slab", &PhysicsBody::ITEM),
        ("falling_sand", &PhysicsBody::FALLING_BLOCK),
    ]);
}

fn assert_wet_ground_case(name: &str) {
    static CAPTURE: OnceLock<Value> = OnceLock::new();
    let capture = CAPTURE.get_or_init(|| {
        serde_json::from_str(include_str!("../testdata/bds-1.26.51-wet-ground.json")).unwrap()
    });
    for sample in capture["cases"][name].as_array().unwrap() {
        let blocks = captured_blocks(&sample["blocks"]);
        assert_native_step(
            &blocks,
            &PhysicsBody::ITEM,
            &sample["before"],
            &sample["after"],
            sample["age"].as_u64().unwrap(),
            name,
        );
    }
}

#[test]
fn bds_wet_ground_water_bypasses_dry_throttle() {
    assert_wet_ground_case("ground_air");
    assert_wet_ground_case("ground_thin_water");
}

#[test]
fn bds_wet_ground_lava_forces_precede_throttled_move() {
    assert_wet_ground_case("ground_lava_depth8_input");
}
