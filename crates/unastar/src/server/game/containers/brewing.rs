//! Brewing stands: slots 0 ingredient, 1..=3 bottles, 4 fuel.
//!
//! Measured on BDS 1.26.51 (client-diff `brewing`, see the items research
//! notes):
//! - Opening sends `ContainerSetData` ids 0 brew time, 1 fuel left and
//!   2 fuel total (zeros included) before `ContainerOpen` (type 4); later
//!   changes go out as they happen.
//! - Bottle slots take potions, splash and lingering potions only (a glass
//!   bottle is refused).
//! - A brew starts in the tick a brew becomes possible (the ingredient
//!   brews at least one bottle) and fuel is left, or blaze powder is there
//!   to refuel: the powder is used up at once for [`BLAZE_POWDER_BREWS`]
//!   brews (public reference count, matching the measured 20). `CookTime`
//!   counts down from [`BREW_TICKS`], one per tick.
//! - At zero every brewable bottle becomes its result carrying
//!   `{wasJustBrewed: 1}` (CraftingData potion mixes and container changes,
//!   see [`RecipeRegistry::brewing_result`]), one ingredient is used and one
//!   unit of fuel is spent (fuel drops at completion, not at the start).
//!   Viewers get `InventorySlot` for each changed bottle slot then the
//!   ingredient slot, and `potion.brewed` plays at the stand's centre the
//!   next tick (data -1).
//! - The block's `brewing_stand_slot_{a,b,c}_bit` show which bottle slots
//!   (1, 2, 3) are filled.
//!
//! Public reference: a brew is cancelled as soon as nothing would brew, or
//! when the ingredient is swapped for a different item.

use std::collections::HashMap;

use bevy_ecs::prelude::*;
use glam::{DVec3, IVec3};
use unastar_data::recipes::RecipeRegistry;
use zuri_nbt::{NBTTag, tag};

use super::{
    BlockContainer, BlockRegistryResource, RecipeRegistryResource, StateValue, container,
    container_mut, queue_sound, save, send_window_slots,
};
use crate::block_entity::{BLAZE_POWDER_BREWS, BREW_TICKS};
use crate::entity::components::Inventory;
use crate::item::ItemStack;

pub(super) const INGREDIENT: usize = 0;
pub(super) const BOTTLES: [usize; 3] = [1, 2, 3];
pub(super) const FUEL: usize = 4;
/// `ContainerSetData` ids a brewing stand window uses.
pub(super) const DATA_IDS: usize = 3;
const FUEL_ITEM: &str = "minecraft:blaze_powder";
const BOTTLE_BITS: [&str; 3] = [
    "brewing_stand_slot_a_bit",
    "brewing_stand_slot_b_bit",
    "brewing_stand_slot_c_bit",
];

/// Whether `stack` fuels a brewing stand.
pub(crate) fn is_fuel(stack: &ItemStack) -> bool {
    stack.item_id() == FUEL_ITEM
}

/// Whether `stack` may sit in a bottle slot.
pub(crate) fn is_bottle(stack: &ItemStack) -> bool {
    unastar_data::recipes::POTION_CONTAINERS.contains(&stack.item_id())
}

/// The potion `ingredient` turns `bottle` into, as item name and aux.
fn brewed<'a>(
    recipes: &'a RecipeRegistry,
    ingredient: &ItemStack,
    bottle: &ItemStack,
) -> Option<(&'a str, i16)> {
    if ingredient.is_empty() || bottle.is_empty() {
        return None;
    }
    recipes
        .brewing_result(ingredient.item_id(), bottle.item_id(), bottle.damage)
        .map(|potion| (potion.item, potion.aux))
}

/// Whether the ingredient brews at least one of the bottles.
pub(super) fn can_brew(recipes: &RecipeRegistry, slots: &Inventory) -> bool {
    let ingredient = slots.item(INGREDIENT).expect("brewing stand slots");
    BOTTLES.into_iter().any(|slot| {
        brewed(
            recipes,
            ingredient,
            slots.item(slot).expect("brewing stand slots"),
        )
        .is_some()
    })
}

/// Brew every brewable bottle and use one ingredient; returns the bottle
/// slots that changed.
fn brew(world: &World, recipes: &RecipeRegistry, slots: &mut Inventory) -> Vec<usize> {
    let ingredient = slots.item(INGREDIENT).expect("brewing stand slots").clone();
    let just_brewed = tag::Compound(HashMap::from([(
        "wasJustBrewed".to_owned(),
        NBTTag::Byte(tag::Byte(1)),
    )]));
    let mut changed = Vec::with_capacity(BOTTLES.len());
    for slot in BOTTLES {
        let bottle = slots.item(slot).expect("brewing stand slots");
        let Some((item, aux)) = brewed(recipes, &ingredient, bottle) else {
            continue;
        };
        match super::super::items::world_stack(world, item, 1) {
            Some(potion) => {
                let potion = crate::item::enchant::with_item_tag(
                    &potion.with_damage(aux),
                    just_brewed.clone(),
                );
                let _ = slots.set_item(slot, potion.into_slot());
                changed.push(slot);
            }
            None => tracing::warn!(item, "brewing result is not in the item registry"),
        }
    }
    let _ = slots.set_item(INGREDIENT, ingredient.grow(-1));
    changed
}

/// One brewing stand tick (see the module rules).
pub(super) fn tick(world: &mut World, pos: IVec3) {
    let Some(recipes) = world
        .get_resource::<RecipeRegistryResource>()
        .map(|recipes| recipes.0.clone())
    else {
        return;
    };
    let Some(snapshot) = container(world, pos).cloned() else {
        return;
    };
    let mut stand = snapshot.clone();
    // Slots whose change viewers are told with `InventorySlot`, in order.
    let mut changed = Vec::new();
    let mut completed = false;
    let brewable = can_brew(&recipes, &stand.items);
    if stand.brewing.brew_time > 0 {
        if !brewable {
            stand.brewing.brew_time = 0;
        } else {
            stand.brewing.brew_time -= 1;
            if stand.brewing.brew_time == 0 {
                changed = brew(world, &recipes, &mut stand.items);
                changed.push(INGREDIENT);
                stand.brewing.fuel_amount -= 1;
                completed = true;
            }
        }
    } else if brewable {
        let fuel = stand.items.item(FUEL).expect("brewing stand slots").clone();
        if stand.brewing.fuel_amount <= 0 && is_fuel(&fuel) {
            stand.brewing.fuel_amount = BLAZE_POWDER_BREWS;
            stand.brewing.fuel_total = BLAZE_POWDER_BREWS;
            let _ = stand.items.set_item(FUEL, fuel.grow(-1));
            changed.push(FUEL);
        }
        if stand.brewing.fuel_amount > 0 {
            stand.brewing.brew_time = BREW_TICKS;
        }
    }
    if stand.brewing == snapshot.brewing && changed.is_empty() {
        return;
    }
    if let Some(mut live) = container_mut(world, pos) {
        *live = stand;
    }
    save(world, pos);
    if !changed.is_empty() {
        sync_bottle_bits(world, pos);
        send_window_slots(world, pos, &changed);
    }
    if completed {
        let centre = pos.as_dvec3() + DVec3::splat(0.5);
        queue_sound(world, 1, centre, "potion.brewed", -1);
    }
}

/// A window request changed the stand's slots: a different ingredient
/// cancels the running brew.
pub(super) fn committed(world: &mut World, pos: IVec3, before: &Inventory, after: &Inventory) {
    let (old, new) = (
        before.item(INGREDIENT).expect("brewing stand slots"),
        after.item(INGREDIENT).expect("brewing stand slots"),
    );
    let same = old.is_empty() && new.is_empty() || old.item_type() == new.item_type();
    if !same && let Some(mut stand) = container_mut(world, pos) {
        stand.brewing.brew_time = 0;
    }
}

/// Set the block's bottle bits from the bottle slots.
pub(super) fn sync_bottle_bits(world: &mut World, pos: IVec3) {
    let Some(filled) = container(world, pos).map(|stand: &BlockContainer| {
        BOTTLES.map(|slot| {
            !stand
                .items
                .item(slot)
                .expect("brewing stand slots")
                .is_empty()
        })
    }) else {
        return;
    };
    let Some(runtime_id) = super::super::blocks::block_at(world, (pos.x, pos.y, pos.z)) else {
        return;
    };
    let blocks = world.resource::<BlockRegistryResource>().0.clone();
    let state = blocks.get_by_runtime_id(runtime_id).and_then(|entry| {
        BOTTLE_BITS
            .into_iter()
            .zip(filled)
            .try_fold(runtime_id, |state, (bit, filled)| {
                entry.with_state_value(state, bit, StateValue::Byte(u8::from(filled)))
            })
    });
    let Some(state) = state else {
        tracing::warn!(?pos, "brewing stand block has no bottle bits");
        return;
    };
    if state != runtime_id {
        super::super::blocks::set_block(world, pos, state, false);
        save(world, pos);
    }
}
