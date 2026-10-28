//! Items dropped by broken blocks.
//!
//! BDS 1.26.51 ships no loot tables for most blocks (their drops are built
//! into the server); unastar expresses them as the `blocks/*` loot tables of
//! `unastar-data` (see `data/overrides/loot_tables/blocks.json`, authored
//! from the Java Edition tables until a BDS drop sweep replaces them).
//! Resolution order for a broken block:
//!
//! 1. A block that requires the correct tool drops nothing without it
//!    (`requiresCorrectToolForDrops`, [`super::mining::can_harvest`]).
//! 2. The upper half of a two-block structure (`upper_block_bit` set: doors,
//!    tall plants) drops nothing; the lower half carries the drop.
//! 3. `blocks/<name>` loot table if one exists.
//! 4. Otherwise the block's own item.

use bevy_ecs::prelude::*;
use glam::{DVec3, IVec3};
use rand::Rng;
use tracing::warn;

use super::items::world_stack;
use super::loot::LootContext;
use super::mining::can_harvest;
use super::types::BlockRegistryResource;
use crate::entity::components::PhysicsBody;
use crate::entity::{EntityRandom, spawn_item_drop};
use crate::item::ItemStack;
use crate::registry::block::StateValue;
use unastar_rules::loot::{LootDrop, roll};

/// Spawn `stack` as an item popping out of the block at `pos`: the item's
/// centre lands within ±0.25 of the block centre (dragonfly
/// `world.dropItem`-style scatter).
pub(crate) fn pop_resource(world: &mut World, pos: IVec3, stack: ItemStack) -> Option<Entity> {
    let offset = {
        let mut rng = world.get_resource_or_init::<EntityRandom>();
        DVec3::new(
            rng.0.gen_range(-0.25..0.25),
            rng.0.gen_range(-0.25..0.25),
            rng.0.gen_range(-0.25..0.25),
        )
    };
    let centre = pos.as_dvec3() + DVec3::splat(0.5) + offset;
    let feet = centre - DVec3::new(0.0, PhysicsBody::ITEM.height / 2.0, 0.0);
    spawn_item_drop(world, feet, stack, None)
}

/// Item stacks breaking `block_runtime_id` with `tool` yields.
pub(crate) fn block_drops(
    world: &mut World,
    block_runtime_id: u32,
    tool: Option<&ItemStack>,
) -> Vec<ItemStack> {
    let Some(registry) = world
        .get_resource::<BlockRegistryResource>()
        .map(|r| r.0.clone())
    else {
        return Vec::new();
    };
    let Some(block) = registry.get_by_runtime_id(block_runtime_id) else {
        return Vec::new();
    };
    if !can_harvest(block, tool) {
        return Vec::new();
    }
    if block.state_value(block_runtime_id, "upper_block_bit") == Some(StateValue::Byte(1)) {
        return Vec::new();
    }
    let name = block
        .string_id
        .strip_prefix("minecraft:")
        .unwrap_or(&block.string_id);
    let Some(table) = unastar_data::loot::get(&format!("blocks/{name}")) else {
        return world_stack(world, &block.string_id, 1)
            .into_iter()
            .collect();
    };
    let ctx = LootContext {
        tool,
        block: Some((block, block_runtime_id)),
        ..LootContext::default()
    };
    let rolled = {
        let mut rng = world.get_resource_or_init::<EntityRandom>();
        roll(unastar_data::loot::vanilla(), table, &ctx, &mut rng.0)
    };
    match rolled {
        Ok(drops) => loot_stacks(world, drops),
        Err(error) => {
            warn!(table = table.name, %error, "Block loot table cannot be evaluated");
            Vec::new()
        }
    }
}

/// Item stacks for rolled loot, oversized counts split into full stacks.
pub(crate) fn loot_stacks(world: &World, drops: Vec<LootDrop>) -> Vec<ItemStack> {
    let mut stacks = Vec::new();
    for drop in drops {
        let Some(template) = world_stack(world, &drop.item_id, 1) else {
            warn!(item = drop.item_id, "Loot item is not in the item registry");
            continue;
        };
        let mut template = template.with_damage(drop.data);
        if drop.nbt.is_some() {
            template.nbt = drop.nbt;
        }
        let mut left = drop.count;
        while left > 0 {
            let take = left.min(u32::from(template.max_stack_size()));
            let mut stack = template.clone();
            stack.count = take as u8;
            stacks.push(stack);
            left -= take;
        }
    }
    stacks
}

/// Drop what breaking `block_runtime_id` at `pos` with `tool` yields.
pub(crate) fn drop_block_items(
    world: &mut World,
    pos: IVec3,
    block_runtime_id: u32,
    tool: Option<&ItemStack>,
) {
    for stack in block_drops(world, block_runtime_id, tool) {
        pop_resource(world, pos, stack);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::item::enchant::{Enchantment, with_enchantments};
    use crate::registry::{BlockRegistry, ItemRegistry};
    use crate::server::game::types::ItemRegistryResource;
    use std::sync::Arc;

    fn world() -> World {
        let mut world = World::new();
        let mut items = ItemRegistry::new();
        items.load_vanilla();
        world.insert_resource(ItemRegistryResource(Arc::new(items)));
        world.insert_resource(BlockRegistryResource(Arc::new(
            BlockRegistry::vanilla().clone(),
        )));
        world.insert_resource(EntityRandom::seeded(11));
        world
    }

    fn state(name: &str) -> u32 {
        BlockRegistry::vanilla()
            .default_state_id_by_name(name)
            .unwrap()
    }

    fn ids(stacks: &[ItemStack]) -> Vec<(&str, u8)> {
        stacks
            .iter()
            .map(|s| (s.item_id.as_str(), s.count))
            .collect()
    }

    #[test]
    fn stone_needs_a_pickaxe_and_drops_cobblestone() {
        let mut world = world();
        let pick = ItemStack::new("minecraft:wooden_pickaxe", 1);
        assert!(block_drops(&mut world, state("minecraft:stone"), None).is_empty());
        assert!(
            block_drops(
                &mut world,
                state("minecraft:stone"),
                Some(&ItemStack::new("minecraft:iron_shovel", 1))
            )
            .is_empty()
        );
        assert_eq!(
            ids(&block_drops(
                &mut world,
                state("minecraft:stone"),
                Some(&pick)
            )),
            vec![("minecraft:cobblestone", 1)]
        );
        let silk = with_enchantments(&pick, &[(Enchantment::SilkTouch, 1)]);
        assert_eq!(
            ids(&block_drops(
                &mut world,
                state("minecraft:stone"),
                Some(&silk)
            )),
            vec![("minecraft:stone", 1)]
        );
    }

    #[test]
    fn ores_drop_items_and_tier_gates_them() {
        let mut world = world();
        let stone_pick = ItemStack::new("minecraft:stone_pickaxe", 1);
        let iron_pick = ItemStack::new("minecraft:iron_pickaxe", 1);
        assert_eq!(
            ids(&block_drops(
                &mut world,
                state("minecraft:iron_ore"),
                Some(&stone_pick)
            )),
            vec![("minecraft:raw_iron", 1)]
        );
        assert!(
            block_drops(
                &mut world,
                state("minecraft:diamond_ore"),
                Some(&stone_pick)
            )
            .is_empty()
        );
        assert_eq!(
            ids(&block_drops(
                &mut world,
                state("minecraft:diamond_ore"),
                Some(&iron_pick)
            )),
            vec![("minecraft:diamond", 1)]
        );
        assert_eq!(
            ids(&block_drops(
                &mut world,
                state("minecraft:coal_ore"),
                Some(&iron_pick)
            )),
            vec![("minecraft:coal", 1)]
        );
    }

    #[test]
    fn default_rule_tool_free_blocks_and_upper_halves() {
        let mut world = world();
        assert_eq!(
            ids(&block_drops(&mut world, state("minecraft:dirt"), None)),
            vec![("minecraft:dirt", 1)]
        );
        assert_eq!(
            ids(&block_drops(
                &mut world,
                state("minecraft:grass_block"),
                None
            )),
            vec![("minecraft:dirt", 1)]
        );
        assert!(block_drops(&mut world, state("minecraft:glass"), None).is_empty());
        let door = BlockRegistry::vanilla()
            .get_by_name("minecraft:wooden_door")
            .unwrap();
        let upper = door
            .with_state_value(
                door.default_state_id,
                "upper_block_bit",
                StateValue::Byte(1),
            )
            .unwrap();
        assert!(block_drops(&mut world, upper, None).is_empty());
        assert_eq!(
            ids(&block_drops(&mut world, door.default_state_id, None)),
            vec![("minecraft:wooden_door", 1)]
        );
        assert_eq!(
            ids(&block_drops(
                &mut world,
                state("minecraft:lit_furnace"),
                Some(&ItemStack::new("minecraft:wooden_pickaxe", 1))
            )),
            vec![("minecraft:furnace", 1)]
        );
    }

    #[test]
    fn dropped_stacks_carry_registry_data() {
        let mut world = world();
        let pick = ItemStack::new("minecraft:iron_pickaxe", 1);
        let stacks = block_drops(&mut world, state("minecraft:stone"), Some(&pick));
        let cobble = &stacks[0];
        assert_eq!(cobble.max_stack_size(), 64);
        assert_eq!(
            u32::try_from(cobble.runtime_id).unwrap(),
            state("minecraft:cobblestone")
        );
    }
}
