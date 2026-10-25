//! Survivability of placed blocks (`enforce_survivability_rules` /
//! `enforce_placement_rules`).
//!
//! BDS keeps these rules in each block's engine behaviour, not in pack data.
//! They are reproduced here as build-time classes over block names and the
//! native block tags, so the emitted placement code tests the supporting
//! block with a compiled `matches!`. Every class states the vanilla rule it
//! encodes; blocks outside every class have no support rule.

use crate::blocks::{BlockPalette, IdSet, StateValue};
use crate::error::Result;

/// What a placed block needs to stay.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Survival {
    /// No support rule.
    Any,
    /// The block below must be in the set.
    Below(IdSet),
    /// The block above must be in the set.
    Above(IdSet),
}

/// Plants that grow on the `dirt` block tag (grass_block, dirt, podzol, moss, mud, ...).
const DIRT_PLANTS: &[&str] = &[
    "minecraft:short_grass",
    "minecraft:fern",
    "minecraft:tall_grass",
    "minecraft:large_fern",
    "minecraft:bush",
    "minecraft:sunflower",
    "minecraft:lilac",
    "minecraft:rose_bush",
    "minecraft:peony",
    "minecraft:dandelion",
    "minecraft:poppy",
    "minecraft:blue_orchid",
    "minecraft:allium",
    "minecraft:azure_bluet",
    "minecraft:red_tulip",
    "minecraft:orange_tulip",
    "minecraft:white_tulip",
    "minecraft:pink_tulip",
    "minecraft:oxeye_daisy",
    "minecraft:cornflower",
    "minecraft:lily_of_the_valley",
    "minecraft:torchflower",
    "minecraft:open_eyeblossom",
    "minecraft:closed_eyeblossom",
    "minecraft:pink_petals",
    "minecraft:wildflowers",
    "minecraft:leaf_litter",
    "minecraft:sweet_berry_bush",
    "minecraft:firefly_bush",
    "minecraft:oak_sapling",
    "minecraft:spruce_sapling",
    "minecraft:birch_sapling",
    "minecraft:jungle_sapling",
    "minecraft:acacia_sapling",
    "minecraft:dark_oak_sapling",
    "minecraft:cherry_sapling",
    "minecraft:pale_oak_sapling",
];

/// Dry plants that also grow on sand and terracotta.
const DRY_PLANTS: &[&str] = &[
    "minecraft:deadbush",
    "minecraft:short_dry_grass",
    "minecraft:tall_dry_grass",
    "minecraft:red_shrub",
];

/// Blocks hanging from a solid ceiling.
const CEILING_PLANTS: &[&str] = &["minecraft:spore_blossom", "minecraft:hanging_roots"];

const DRY_GROUND_TAGS: &[&str] = &["dirt", "sand"];

fn terracotta(palette: &BlockPalette) -> Vec<&str> {
    palette
        .names()
        .filter(|n| {
            *n == "minecraft:hardened_clay" || n.ends_with("_terracotta") && !n.contains("glazed")
        })
        .collect()
}

/// Survival class of the block placed with runtime id `id`.
pub fn classify(palette: &BlockPalette, origin: &str, id: u32) -> Result<Survival> {
    let Some((name, first, state)) = palette.describe(id) else {
        return Ok(Survival::Any);
    };
    // The upper half of a double plant stands on its own lower half.
    if state.get("upper_block_bit") == Some(&StateValue::Int(1)) {
        let len = palette.states_of(name).map(Iterator::count).unwrap_or(0) as u32;
        return Ok(Survival::Below(IdSet::from_ids(first..first + len)));
    }
    if DIRT_PLANTS.contains(&name) {
        return Ok(Survival::Below(palette.matching(
            origin,
            &crate::blocks::BlockPredicate::AnyTag(vec!["dirt".into()]),
        )?));
    }
    if DRY_PLANTS.contains(&name) {
        let tags = DRY_GROUND_TAGS.iter().map(|t| t.to_string()).collect();
        let ground = palette.matching(origin, &crate::blocks::BlockPredicate::AnyTag(tags))?;
        return Ok(Survival::Below(
            ground.union(&palette.all_states(origin, &terracotta(palette))?),
        ));
    }
    if CEILING_PLANTS.contains(&name) {
        return Ok(Survival::Above(palette.ids_where(|f| f.solid)));
    }
    Ok(Survival::Any)
}
