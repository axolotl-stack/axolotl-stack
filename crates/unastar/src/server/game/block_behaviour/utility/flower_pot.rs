//! Flower pots (`update_bit`: set while a plant is in the pot; the plant
//! lives in the `FlowerPot` block actor as `PlantBlock`, a `{name, states,
//! version}` block-state compound written only while a plant is in the
//! pot).
//!
//! - **Use** (always handled): holding a pottable plant over an empty pot
//!   puts the plant's block in (its default state); the plant item is used
//!   up outside creative. Holding anything else over a filled pot takes the
//!   plant out into the player's inventory (dropped at their feet when it
//!   does not fit). A pottable plant held over a filled pot changes
//!   nothing. Pots play no sound.
//! - **Pottable plants** ([`is_pottable`]): the small flowers, saplings and
//!   the mangrove propagule, red and brown mushrooms, crimson and warped
//!   fungi and roots, cacti, bamboo, ferns, dead bushes, azaleas and
//!   flowering azaleas (provisional: Minecraft Wiki; the vanilla list is
//!   block data the server does not ship).
//! - **Survival**: the block below must hold the middle of its top face
//!   (provisional: Minecraft Wiki); the core breaks an unsupported pot one
//!   tick after the neighbour change.
//! - **Breaking**: a player breaking a filled pot also drops its plant.
//!
//! Not modelled: a potted eyeblossom opening and closing with the time of
//! day (random ticks do not reach pots).

use std::collections::HashMap;

use bevy_ecs::prelude::*;
use glam::IVec3;
use zuri_nbt::{NBTTag, tag};

use super::super::{Place, SupportKind, actors, bit, provides_support, with};
use crate::entity::components::GameMode;
use crate::entity::systems::BlockSource;
use crate::registry::BlockRegistry;
use crate::registry::block::{BlockEntry, StateValue};
use crate::server::game::item_use::{give_or_drop, held, set_slot};
use crate::server::game::types::{BlockRegistryResource, ItemRegistryResource};

/// The pot's block actor id.
pub const ACTOR: &str = "FlowerPot";
/// The actor key holding the plant.
pub const PLANT: &str = "PlantBlock";
/// The pot's filled flag.
pub const UPDATE: &str = "update_bit";
/// Block-state version written with the plant (as the server writes it).
const BLOCK_VERSION: i32 = 18_168_865;

/// Whether the block `name` can be potted.
pub fn is_pottable(name: &str) -> bool {
    matches!(
        name.strip_prefix("minecraft:").unwrap_or(name),
        "poppy"
            | "dandelion"
            | "blue_orchid"
            | "allium"
            | "azure_bluet"
            | "red_tulip"
            | "orange_tulip"
            | "white_tulip"
            | "pink_tulip"
            | "oxeye_daisy"
            | "cornflower"
            | "lily_of_the_valley"
            | "wither_rose"
            | "torchflower"
            | "open_eyeblossom"
            | "closed_eyeblossom"
            | "oak_sapling"
            | "spruce_sapling"
            | "birch_sapling"
            | "jungle_sapling"
            | "acacia_sapling"
            | "dark_oak_sapling"
            | "cherry_sapling"
            | "pale_oak_sapling"
            | "mangrove_propagule"
            | "red_mushroom"
            | "brown_mushroom"
            | "crimson_fungus"
            | "warped_fungus"
            | "crimson_roots"
            | "warped_roots"
            | "cactus"
            | "bamboo"
            | "fern"
            | "deadbush"
            | "azalea"
            | "flowering_azalea"
    )
}

/// The plant stored in the pot at `pos`.
pub fn plant(world: &World, pos: IVec3) -> Option<u32> {
    let actor = actors::read_kind(world, pos, ACTOR)?;
    let NBTTag::Compound(block) = actor.0.get(PLANT)? else {
        return None;
    };
    let registry = &world.resource::<BlockRegistryResource>().0;
    block_from_nbt(registry, block).filter(|state| {
        registry
            .get_by_runtime_id(*state)
            .is_some_and(|entry| entry.string_id != "minecraft:air")
    })
}

/// `{name, states, version}` of the block state `state`.
fn block_nbt(entry: &BlockEntry, state: u32) -> tag::Compound {
    let states = entry
        .data
        .properties
        .iter()
        .filter_map(|property| {
            let value = match entry.state_value(state, property.name)? {
                StateValue::Byte(value) => NBTTag::Byte(tag::Byte(value)),
                StateValue::Int(value) => NBTTag::Int(tag::Int(value)),
                StateValue::Str(value) => NBTTag::String(tag::String(value.into())),
            };
            Some((property.name.to_owned(), value))
        })
        .collect();
    tag::Compound(HashMap::from([
        (
            "name".to_owned(),
            NBTTag::String(tag::String(entry.string_id.clone())),
        ),
        ("states".to_owned(), NBTTag::Compound(tag::Compound(states))),
        ("version".to_owned(), NBTTag::Int(tag::Int(BLOCK_VERSION))),
    ]))
}

/// The block state a `{name, states}` compound names.
fn block_from_nbt(registry: &BlockRegistry, block: &tag::Compound) -> Option<u32> {
    let NBTTag::String(name) = block.0.get("name")? else {
        return None;
    };
    let entry = registry.get_by_name(&name.0)?;
    let mut state = entry.default_state_id;
    if let Some(NBTTag::Compound(states)) = block.0.get("states") {
        for property in entry.data.properties {
            let value = match states.0.get(property.name) {
                Some(NBTTag::Byte(value)) => StateValue::Byte(value.0),
                Some(NBTTag::Int(value)) => StateValue::Int(value.0),
                Some(NBTTag::String(value)) => *property
                    .values
                    .iter()
                    .find(|known| matches!(known, StateValue::Str(s) if *s == value.0))?,
                _ => continue,
            };
            state = with(entry, state, property.name, value);
        }
    }
    Some(state)
}

/// Store `plant` (a block state, or nothing) in the pot at `pos` and show
/// whether it is filled with `update_bit`.
fn store_plant(world: &mut World, pos: IVec3, plant: Option<u32>) {
    let mut actor = actors::new_compound(ACTOR, pos);
    let registry = world.resource::<BlockRegistryResource>().0.clone();
    if let Some(state) = plant
        && let Some(entry) = registry.get_by_runtime_id(state)
    {
        actor
            .0
            .insert(PLANT.into(), NBTTag::Compound(block_nbt(entry, state)));
    }
    actors::store(world, pos, actor);
    let Some(state) = world.block_at(pos) else {
        return;
    };
    let Some(entry) = registry.get_by_runtime_id(state) else {
        return;
    };
    let filled = plant.is_some();
    if bit(entry, state, UPDATE) != filled {
        let next = with(entry, state, UPDATE, StateValue::Byte(u8::from(filled)));
        crate::server::game::blocks::set_block(world, pos, next, false);
    }
}

/// The item a potted plant gives back.
fn plant_item(world: &World, plant: u32) -> Option<crate::item::ItemStack> {
    let name = world
        .resource::<BlockRegistryResource>()
        .0
        .get_by_runtime_id(plant)?
        .string_id
        .clone();
    crate::server::game::world_stack(world, &name, 1)
}

/// Whether a player's `use` on `state` is a block interaction: always.
pub fn is_usable(_entry: &BlockEntry, _state: u32) -> bool {
    true
}

/// A player's `use`: pot the held plant, or take the potted one out.
pub fn use_block(world: &mut World, player: Entity, pos: IVec3, _state: u32) -> bool {
    let current = plant(world, pos);
    let held = held(world, player);
    let pottable = held.as_ref().and_then(|(_, stack)| {
        let items = world.resource::<ItemRegistryResource>().0.clone();
        let blocks = world.resource::<BlockRegistryResource>().0.clone();
        let entry = items.get_by_name(stack.item_id())?;
        let block = super::super::item_block(&blocks, entry)?;
        is_pottable(&block.string_id).then_some(block.default_state_id)
    });
    match (pottable, current) {
        (Some(plant), None) => {
            store_plant(world, pos, Some(plant));
            if let Some((slot, stack)) = held
                && world.get::<GameMode>(player) != Some(&GameMode::Creative)
            {
                set_slot(world, player, slot, stack.grow(-1));
            }
        }
        (None, Some(plant)) => {
            store_plant(world, pos, None);
            if let Some(item) = plant_item(world, plant) {
                give_or_drop(world, player, item);
            }
        }
        _ => {}
    }
    true
}

/// Placement keeps the generic state.
pub fn placement(place: &Place<'_, impl BlockSource>) -> Option<u32> {
    Some(place.state)
}

/// A pot needs the middle of the top face of the block below.
pub fn can_survive(
    blocks: &impl BlockSource,
    pos: IVec3,
    _state: u32,
    _entry: &BlockEntry,
) -> bool {
    provides_support(blocks, pos - IVec3::Y, IVec3::Y, SupportKind::Centre)
}

/// A new pot is empty: nothing to do.
pub fn placed(_world: &mut World, _pos: IVec3, _state: u32) {}

/// Survival is checked by the core; nothing else reacts to neighbours.
pub fn neighbour_changed(_world: &mut World, _pos: IVec3, _state: u32, _from: IVec3) {}

/// Pots schedule no ticks of their own.
pub fn scheduled_tick(_world: &mut World, _pos: IVec3, _state: u32) {}

/// Random ticks do not reach pots (see the module docs).
pub fn random_tick(_world: &mut World, _pos: IVec3, _state: u32) {}

/// Redstone power does not act on pots.
pub fn power_changed(_world: &mut World, _pos: IVec3, _state: u32, _powered: bool) {}

/// A player breaks the pot: its plant drops too.
pub fn breaking(world: &mut World, pos: IVec3, _state: u32) {
    if let Some(plant) = plant(world, pos)
        && let Some(item) = plant_item(world, plant)
    {
        crate::server::game::block_drops::pop_resource(world, pos, item);
    }
}

#[cfg(test)]
mod tests {
    use glam::{DVec3, IVec3};

    use super::super::super::tests::{FLOOR, Harness, state};
    use super::*;

    const AT: IVec3 = IVec3::new(2, FLOOR + 1, 2);
    const TOP: DVec3 = DVec3::new(0.5, 0.4, 0.5);

    fn potted(h: &Harness) -> Option<String> {
        let plant = plant(&h.world, AT)?;
        Some(
            h.world
                .registry()
                .get_by_runtime_id(plant)?
                .string_id
                .clone(),
        )
    }

    fn held_item(h: &Harness) -> Option<(String, u8)> {
        crate::server::game::item_use::held(&h.world, h.player)
            .map(|(_, stack)| (stack.item_id().to_owned(), stack.count))
    }

    #[test]
    fn holds_a_plant_and_gives_it_back() {
        let mut h = Harness::new();
        h.put(AT, state("minecraft:flower_pot", &[]));
        h.hold("minecraft:poppy", 2);
        h.click(AT, 1, TOP);
        assert_eq!(potted(&h).as_deref(), Some("minecraft:poppy"));
        assert_eq!(held_item(&h), Some(("minecraft:poppy".into(), 1)));
        // A second plant does not go into a full pot.
        h.hold("minecraft:dandelion", 1);
        h.click(AT, 1, TOP);
        assert_eq!(potted(&h).as_deref(), Some("minecraft:poppy"));
        assert_eq!(held_item(&h), Some(("minecraft:dandelion".into(), 1)));
        // Anything else takes the plant out.
        h.hold("minecraft:stick", 1);
        h.click(AT, 1, TOP);
        assert_eq!(potted(&h), None);
        let inventory = &h
            .world
            .get::<crate::entity::components::MainInventory>(h.player)
            .unwrap()
            .0;
        let poppies: u8 = inventory
            .slots()
            .iter()
            .filter(|stack| stack.item_id() == "minecraft:poppy")
            .map(|stack| stack.count)
            .sum();
        assert_eq!(poppies, 1);
    }

    #[test]
    fn only_plants_go_in() {
        let mut h = Harness::new();
        h.put(AT, state("minecraft:flower_pot", &[]));
        h.hold("minecraft:stone", 1);
        h.click(AT, 1, TOP);
        assert_eq!(potted(&h), None);
        assert_eq!(held_item(&h), Some(("minecraft:stone".into(), 1)));
        h.hold("minecraft:oak_sapling", 1);
        h.click(AT, 1, TOP);
        assert_eq!(potted(&h).as_deref(), Some("minecraft:oak_sapling"));
    }

    #[test]
    fn breaking_a_filled_pot_drops_the_plant() {
        let mut h = Harness::new();
        h.put(AT, state("minecraft:flower_pot", &[]));
        h.hold("minecraft:cactus", 1);
        h.click(AT, 1, TOP);
        h.break_at(AT);
        assert_eq!(h.name(AT), "minecraft:air");
        assert_eq!(
            h.drops(),
            vec![
                ("minecraft:cactus".to_string(), 1),
                ("minecraft:flower_pot".to_string(), 1),
            ]
        );
    }
}
