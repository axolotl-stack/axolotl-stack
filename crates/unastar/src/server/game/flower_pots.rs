//! Flower pots: a click puts a plant in or takes it out (BDS 1.26.51,
//! `testdata/bds-placements-leftovers.json` and `-actors.json`).
//!
//! - An empty pot takes one of the held stack if it is one of [`PLANTS`]: the
//!   pot's `update_bit` turns on and its actor gets a `PlantBlock` (the
//!   plant's block in its default state). Any other click on an empty pot
//!   does nothing: nothing is placed against the pot either.
//! - A pot holding a plant gives it back to a click with anything that is not
//!   a plant (an empty hand gets it in the hand, a block in hand sends it to
//!   the inventory); a click with a plant does nothing.
//! - BDS sends the click corrections before it changes the pot.

use bevy_ecs::prelude::*;
use glam::IVec3;
use zuri_nbt::{NBTTag, tag};

use super::block_actors;
use super::types::BlockRegistryResource;
use crate::registry::block::{BlockEntry, StateValue};

/// The items an empty pot takes: 37 of the 80 items tried, each going in as
/// the default state of the block of the same name (`pot_*` cases of
/// `testdata/bds-placements-leftovers.json`). Short grass, bushes, tall
/// plants, petals, seeds and the water and cave plants stay out.
pub(crate) const PLANTS: &[&str] = &[
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
    "minecraft:wither_rose",
    "minecraft:torchflower",
    "minecraft:open_eyeblossom",
    "minecraft:closed_eyeblossom",
    "minecraft:oak_sapling",
    "minecraft:spruce_sapling",
    "minecraft:birch_sapling",
    "minecraft:jungle_sapling",
    "minecraft:acacia_sapling",
    "minecraft:dark_oak_sapling",
    "minecraft:cherry_sapling",
    "minecraft:pale_oak_sapling",
    "minecraft:mangrove_propagule",
    "minecraft:azalea",
    "minecraft:flowering_azalea",
    "minecraft:brown_mushroom",
    "minecraft:red_mushroom",
    "minecraft:crimson_fungus",
    "minecraft:warped_fungus",
    "minecraft:crimson_roots",
    "minecraft:warped_roots",
    "minecraft:fern",
    "minecraft:deadbush",
    "minecraft:cactus",
    "minecraft:bamboo",
];

/// A click on the block at `pos` with `face`: `None` when it is not a flower
/// pot, else `Some(false)` (the pot answered; any corrections were sent).
pub(crate) fn used(world: &mut World, player: Entity, pos: IVec3, face: u8) -> Option<bool> {
    let registry = world.get_resource::<BlockRegistryResource>()?.0.clone();
    let state = super::blocks::block_at(world, (pos.x, pos.y, pos.z))?;
    let pot = registry.get_by_runtime_id(state)?;
    if pot.string_id != "minecraft:flower_pot" {
        return None;
    }
    let mut actor = block_actors::read(world, pos)
        .or_else(|| block_actors::default_compound(&pot.string_id, pos))?;
    let held = held_stack(world, player);
    let plant = held
        .as_deref()
        .filter(|item| PLANTS.contains(item))
        .and_then(|item| registry.get_by_name(item));
    let potted = actor
        .0
        .get("PlantBlock")
        .and_then(|plant| match plant {
            NBTTag::Compound(plant) => plant.0.get("name"),
            _ => None,
        })
        .and_then(|name| match name {
            NBTTag::String(name) => Some(name.0.clone()),
            _ => None,
        });
    match (potted, plant) {
        (None, Some(plant)) => {
            super::blocks::click_corrections(world, player, pos, face);
            block_actors::consume_held(world, player);
            actor.0.insert(
                "PlantBlock".into(),
                plant_block(plant, plant.default_state_id),
            );
            set_pot(world, pos, pot, state, true, actor);
        }
        (Some(name), None) => {
            super::blocks::click_corrections(world, player, pos, face);
            actor.0.remove("PlantBlock");
            set_pot(world, pos, pot, state, false, actor);
            if let Some(stack) = world
                .get_resource::<super::types::ItemRegistryResource>()
                .and_then(|items| items.0.stack(&name, 1))
            {
                super::item_use::give_or_drop(world, player, stack);
            }
        }
        _ => {}
    }
    Some(false)
}

/// The identifier of the stack in `player`'s selected hotbar slot.
fn held_stack(world: &World, player: Entity) -> Option<String> {
    use crate::entity::components::{HeldSlot, MainInventory};
    let slot = world.get::<HeldSlot>(player)?.0;
    let stack = world.get::<MainInventory>(player)?.hotbar(slot)?;
    (!stack.is_empty()).then(|| stack.item_id().to_owned())
}

/// Store the pot with `update_bit` and its actor, and send both.
fn set_pot(
    world: &mut World,
    pos: IVec3,
    pot: &BlockEntry,
    state: u32,
    potted: bool,
    actor: tag::Compound,
) {
    let next = pot
        .with_state_value(state, "update_bit", StateValue::Byte(u8::from(potted)))
        .unwrap_or(state);
    super::blocks::set_block(world, pos, next, false);
    if let Some(stored) = block_actors::write(world, pos, actor) {
        block_actors::broadcast(world, pos, &block_actors::data_packet(pos, &stored), None);
    }
}

/// `PlantBlock`: the plant's block name, its states and the block version.
fn plant_block(plant: &BlockEntry, state: u32) -> NBTTag {
    block_actors::block_tag(plant, state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    /// [`PLANTS`] is exactly the set of items BDS put in an empty pot.
    #[test]
    fn plants_are_the_items_bds_potted() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/server/game/testdata/bds-placements-leftovers.json"
        );
        let fixture: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        let mut potted = Vec::new();
        let mut tried = 0;
        for case in fixture["cases"].as_array().unwrap() {
            let key = case["key"].as_str().unwrap();
            if !key.starts_with("pot_") || key.contains("_then_") {
                continue;
            }
            tried += 1;
            let pot = &case["bds"]["reads"][0];
            if pot[2]["update_bit"] == Value::Bool(true) {
                potted.push(case["hand"]["item"].as_str().unwrap().to_owned());
            }
        }
        assert_eq!(tried, 80);
        let mut expected: Vec<String> = PLANTS.iter().map(|item| (*item).to_owned()).collect();
        expected.sort();
        potted.sort();
        assert_eq!(potted, expected);
    }
}
