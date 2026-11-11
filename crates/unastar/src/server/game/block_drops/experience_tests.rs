//! Experience from broken blocks against BDS 1.26.51.
//!
//! `testdata/bds-block-experience.json` (`tools/bds-sweep/scripts/ore-xp.txt`)
//! and `bds-block-experience-tools.json` (`scripts/ore-xp-2.txt`) list every
//! break of a scripted player against plain BDS 1.26.51 (2026-11-10): the
//! block, the held tool and the values of the experience orbs BDS added
//! (`tools/bds-sweep/experience_table.py` over the capture). Each break must
//! fit [`experience_drop`]: no orbs when it gives none, else a total inside
//! its range, split into the same orbs as [`orb_values`]. The range's ends
//! must be what BDS dropped least and most for every block broken 30 times.

use std::collections::BTreeMap;

use serde::Deserialize;

use super::*;
use crate::entity::xp_orb::orb_values;
use crate::item::enchant::{Enchantment, with_enchantments};
use crate::registry::BlockRegistry;

#[derive(Deserialize)]
struct Table {
    breaks: Vec<Break>,
}

#[derive(Deserialize)]
struct Break {
    block: String,
    tool: String,
    orbs: Vec<u32>,
}

/// The stack a table's tool key names (`netherite_pickaxe+efficiency5`).
pub(super) fn tool(key: &str) -> Option<ItemStack> {
    let mut parts = key.split('+');
    let item = parts.next()?;
    if item == "hand" {
        return None;
    }
    let stack = crate::item::test_support::stack(&format!("minecraft:{item}"), 1);
    let enchantments: Vec<(Enchantment, u8)> = parts
        .map(|part| {
            let split = part.find(|c: char| c.is_ascii_digit()).unwrap();
            let (name, level) = part.split_at(split);
            (
                Enchantment::from_name(name).unwrap_or_else(|| panic!("{name}")),
                level.parse().unwrap(),
            )
        })
        .collect();
    Some(with_enchantments(&stack, &enchantments))
}

#[test]
fn block_experience_matches_the_bds_breaks() {
    let registry = BlockRegistry::vanilla();
    let mut totals: BTreeMap<&str, Vec<u32>> = BTreeMap::new();
    let tables: Vec<Table> = [
        include_str!("testdata/bds-block-experience.json"),
        include_str!("testdata/bds-block-experience-tools.json"),
    ]
    .iter()
    .map(|text| serde_json::from_str(text).unwrap())
    .collect();
    let mut wrong = Vec::new();
    for row in tables.iter().flat_map(|table| &table.breaks) {
        let state = registry.default_state_id_by_name(&row.block).unwrap();
        let block = registry.get_by_runtime_id(state).unwrap();
        let held = tool(&row.tool);
        let total: u32 = row.orbs.iter().sum();
        let mut orbs = row.orbs.clone();
        orbs.sort_unstable_by(|a, b| b.cmp(a));
        match experience_drop(block, held.as_ref()) {
            None if total == 0 => {}
            None => wrong.push(format!("{} {}: BDS dropped {total}", row.block, row.tool)),
            Some(experience) => {
                if !(experience.min..=experience.max).contains(&total) {
                    wrong.push(format!(
                        "{} {}: BDS dropped {total}, outside {experience:?}",
                        row.block, row.tool
                    ));
                }
                if orb_values(total).collect::<Vec<_>>() != orbs {
                    wrong.push(format!(
                        "{} {}: BDS split {total} as {orbs:?}",
                        row.block, row.tool
                    ));
                }
                if !row.tool.contains("silk_touch") {
                    totals.entry(&row.block).or_default().push(total);
                }
            }
        }
    }
    // The range's ends are what BDS dropped, for every block broken 30 times
    // (the spawner's 43 was not seen in 30 breaks).
    for (block, seen) in &totals {
        let state = registry.default_state_id_by_name(block).unwrap();
        let experience =
            block_experience(&registry.get_by_runtime_id(state).unwrap().string_id).unwrap();
        let (least, most) = (*seen.iter().min().unwrap(), *seen.iter().max().unwrap());
        if seen.len() >= 30
            && *block != "minecraft:mob_spawner"
            && (least, most) != (experience.min, experience.max)
        {
            wrong.push(format!(
                "{block}: BDS dropped {least}..={most}, ours {experience:?}"
            ));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
    assert!(
        totals.len() >= 15,
        "{} blocks with experience",
        totals.len()
    );
}

#[test]
fn orbs_start_at_the_blocks_corner_without_x_velocity() {
    let mut world = World::new();
    world.insert_resource(BlockRegistryResource(std::sync::Arc::new(
        BlockRegistry::vanilla().clone(),
    )));
    world.insert_resource(crate::entity::EntityRandom::seeded(9));
    let ore = BlockRegistry::vanilla()
        .default_state_id_by_name("minecraft:diamond_ore")
        .unwrap();
    let pickaxe = crate::item::test_support::stack("minecraft:iron_pickaxe", 1);
    drop_block_experience(&mut world, IVec3::new(1, -59, 0), ore, Some(&pickaxe));
    let mut orbs = world.query::<(
        &crate::entity::xp_orb::XpOrb,
        &crate::entity::components::Position,
        &crate::entity::components::Velocity,
    )>();
    let mut total = 0;
    for (orb, position, velocity) in orbs.iter(&world) {
        total += orb.value;
        assert_eq!(position.0, DVec3::new(0.875, -59.0, 0.0));
        assert_eq!(velocity.0.x, 0.0);
        assert!((0.0..0.4).contains(&velocity.0.y) && (-0.2..0.2).contains(&velocity.0.z));
    }
    assert!((3..=7).contains(&total), "diamond ore dropped {total}");
}
