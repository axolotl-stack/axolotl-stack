//! Every measured fuel through a real furnace, against the BDS 1.26.51 fuel
//! sweep.
//!
//! `unastar-data/data/measured/fuel_burn_ticks.json` holds, for every
//! registry item that lit BDS's furnace, the server ticks from the first
//! tick the block was `lit_furnace` to the first tick it was `furnace`
//! again, with one cobblestone as the input. The same setup runs here
//! through the furnace tick, item by item.

use std::collections::BTreeMap;

use super::*;

const MEASURED: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../unastar-data/data/measured/fuel_burn_ticks.json"
));

#[test]
fn every_measured_fuel_burns_a_furnace_for_its_bds_time() {
    let fuels: BTreeMap<String, u32> =
        serde_json::from_str(MEASURED).expect("measured fuel table decodes");
    assert!(fuels.len() > 250, "only {} fuels", fuels.len());
    with_world(GameMode::Survival, move |h| {
        let furnace = IVec3::new(6, 64, 6);
        let mut wrong = Vec::new();
        for (item, &measured) in &fuels {
            // A fresh furnace actor each time (placing resets the actor).
            h.place(furnace, "minecraft:furnace", &[north()]);
            {
                let mut live = container_mut(h.world(), furnace).unwrap();
                live.items
                    .set_item(0, stack("minecraft:cobblestone", 1).into_slot())
                    .unwrap();
                live.items.set_item(1, stack(item, 1).into_slot()).unwrap();
            }
            h.tick(1);
            if h.block_name(furnace) != "minecraft:lit_furnace" {
                wrong.push(format!("{item}: does not light the furnace"));
                continue;
            }
            let mut lit = 0;
            while h.block_name(furnace) == "minecraft:lit_furnace" && lit <= measured + 1 {
                h.tick(1);
                lit += 1;
            }
            if lit != measured {
                wrong.push(format!("{item}: burns {lit} ticks, BDS {measured}"));
            }
        }
        assert!(wrong.is_empty(), "{}", wrong.join("\n"));
    });
}
