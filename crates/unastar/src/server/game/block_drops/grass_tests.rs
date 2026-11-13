//! Grass and fern drops against BDS 1.26.51.
//!
//! `testdata/bds-grass-seeds.json` is a BDS 1.26.51 drops sweep
//! (`tools/bds-sweep/sweep.py drops`, 2026-11-11) of short grass, fern, tall
//! grass and large fern: 20000 `/loot ... mine` samples for each of the hand,
//! a diamond pickaxe, shears, Silk Touch and Fortune I-III. BDS drops one
//! wheat seed 1 time in 8 (12.48% ± 0.09% over the 480000 samples that can
//! drop one) whatever the tool, Fortune included, and shears give the plant,
//! two separate stacks of one for the double plants.
//!
//! Each cell is rolled 20000 times from a seeded RNG. Both sides must have
//! the same outcomes, and the share of each outcome must agree with BDS's
//! within four standard errors of the difference of two proportions.

use serde::Deserialize;

use super::*;
use crate::item::enchant::with_enchantments;
use crate::registry::{BlockRegistry, ItemRegistry};
use crate::server::game::types::ItemRegistryResource;
use std::collections::BTreeMap;
use std::sync::Arc;
use unastar_rules::ids::Enchantment;

#[derive(Deserialize)]
struct Table {
    cells: Vec<Cell>,
}

#[derive(Deserialize)]
struct Tool {
    item: String,
    /// Enchantment identifier to level.
    enchantments: BTreeMap<String, u8>,
}

impl Tool {
    fn stack(&self) -> ItemStack {
        let levels = self.enchantments.iter().map(|(name, &level)| {
            let enchantment: Enchantment = name.parse().unwrap_or_else(|_| panic!("{name}"));
            (enchantment, level)
        });
        with_enchantments(&crate::item::test_support::stack(&self.item, 1), levels)
    }
}

#[derive(Deserialize)]
struct Cell {
    block: String,
    /// `None`: the hand.
    tool: Option<Tool>,
    samples: u32,
    outcomes: BTreeMap<String, u32>,
}

/// An outcome as the sweep keys it: `name@data` `x` count per stack, joined
/// by `;`, `""` for nothing.
fn outcome(stacks: &[ItemStack]) -> String {
    let mut keys: Vec<String> = stacks
        .iter()
        .map(|s| format!("{}@{}x{}", s.item_id(), s.damage, s.count))
        .collect();
    keys.sort();
    keys.join(";")
}

#[test]
fn grass_and_fern_drops_match_the_bds_sweep() {
    let table: Table = serde_json::from_str(include_str!("testdata/bds-grass-seeds.json")).unwrap();
    let mut world = World::new();
    let mut items = ItemRegistry::new();
    items.load_vanilla();
    world.insert_resource(ItemRegistryResource(Arc::new(items)));
    world.insert_resource(BlockRegistryResource(Arc::new(
        BlockRegistry::vanilla().clone(),
    )));
    let mut wrong = Vec::new();
    for (index, cell) in table.cells.iter().enumerate() {
        world.insert_resource(EntityRandom::seeded(index as u64));
        let state = BlockRegistry::vanilla()
            .default_state_id_by_name(&cell.block)
            .unwrap();
        let held = cell.tool.as_ref().map(Tool::stack);
        let mut ours: BTreeMap<String, u32> = BTreeMap::new();
        for _ in 0..cell.samples {
            *ours
                .entry(outcome(&block_drops(&mut world, state, held.as_ref())))
                .or_default() += 1;
        }
        let name = format!(
            "{} {}",
            cell.block,
            cell.tool.as_ref().map_or("hand", |tool| tool.item.as_str())
        );
        if ours.keys().ne(cell.outcomes.keys()) {
            wrong.push(format!("{name}: BDS {:?}, ours {ours:?}", cell.outcomes));
            continue;
        }
        let n = f64::from(cell.samples);
        for (key, &bds) in &cell.outcomes {
            let (p, q) = (f64::from(bds) / n, f64::from(ours[key]) / n);
            let pooled = (p + q) / 2.0;
            let error = (2.0 * pooled * (1.0 - pooled) / n).sqrt();
            if (p - q).abs() > 4.0 * error {
                wrong.push(format!("{name} {key:?}: BDS {p:.4}, ours {q:.4}"));
            }
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}
