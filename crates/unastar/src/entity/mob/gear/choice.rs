//! Whether a mob wants an item it sees on the ground, and how many it
//! takes: the native `minecraft:shareables` rules for a mob without an
//! inventory.
//!
//! Wanted items are matched against `shareables` in authored order; the
//! first matching entry gives the item's priority (0 is the most wanted)
//! and limits. An item the list does not name is wanted only with
//! `can_pickup_any_item` or `all_items`.
//!
//! - Armour and head-wearable blocks must be listed. An empty armour slot
//!   takes them; a filled one is replaced by a strictly higher priority
//!   (lower number), or by the same item less worn. Armour cursed with
//!   binding is never swapped.
//! - Anything else goes to the hand it prefers (the shield to the off hand,
//!   the rest to the main hand). An empty hand takes it; a hand holding a
//!   stack it can join takes it while the stack is not full (and not past
//!   `max_amount`); otherwise a listed item replaces the held one on the
//!   same priority rule. `singular_pickup` refuses a second copy of what
//!   the hand holds. `consume_item` entries are always wanted.
//!
//! `can_pickup_to_hand_or_equipment` only matters for mobs with an
//! inventory, which no supported mob has.

use bevy_ecs::prelude::*;
use zuri_nbt::NBTTag;

use super::{GearSlot, MobEquipment, held_slot, worn_slot};
use crate::entity::mob::components::ShareablesConfig;
use crate::entity::mob::constants::BINDING_CURSE_ID;
use crate::item::ItemStack;
use crate::item::enchant::{durability_damage, item_tag};

/// Whether two stacks can be one stack.
pub(crate) fn stackable(a: &ItemStack, b: &ItemStack) -> bool {
    a.comparable(b) && a.max_stack_size() > 1
}

/// Same item and data value, ignoring wear.
fn same_item(a: &ItemStack, b: &ItemStack) -> bool {
    !a.is_empty() && a.item_id == b.item_id && a.damage == b.damage
}

fn has_binding_curse(stack: &ItemStack) -> bool {
    let Some(tag) = item_tag(stack) else {
        return false;
    };
    let Some(NBTTag::List(list)) = tag.0.get("ench") else {
        return false;
    };
    list.0.iter().any(|entry| match entry {
        NBTTag::Compound(entry) => {
            matches!(entry.0.get("id"), Some(NBTTag::Short(id)) if id.0 == BINDING_CURSE_ID)
        }
        _ => false,
    })
}

/// The stack in `slot`; an empty stack for a mob without equipment.
fn carried(equipment: Option<&MobEquipment>, slot: GearSlot) -> ItemStack {
    equipment.map_or_else(ItemStack::empty, |e| e.get(slot).clone())
}

/// Whether `new` should replace `existing` in a slot: strictly more wanted,
/// or the same item less worn; an empty slot takes anything when `any`.
fn should_replace(
    existing: &ItemStack,
    new: &ItemStack,
    wants: &ShareablesConfig,
    any: bool,
) -> bool {
    let priority = |stack: &ItemStack| {
        wants
            .entry(&stack.item_id, stack.damage)
            .map(|entry| entry.priority)
    };
    let existing_priority = priority(existing).unwrap_or(i32::MAX);
    if priority(new).is_some_and(|p| p < existing_priority) {
        return true;
    }
    if same_item(existing, new) && durability_damage(new) < durability_damage(existing) {
        return true;
    }
    existing.is_empty() && any
}

/// Whether a hand holding `held` takes `new` without replacing it.
fn hand_takes(new: &ItemStack, held: &ItemStack, max_amount: i32, singular: bool) -> bool {
    if max_amount == 0 {
        return false;
    }
    if held.is_empty() {
        return true;
    }
    if max_amount > 0 && max_amount <= i32::from(held.count) {
        return false;
    }
    if singular && same_item(held, new) {
        return false;
    }
    stackable(held, new) && held.count < held.max_stack_size()
}

/// Whether `mob` wants the item actor stack `item` (`any`: the goal's
/// `can_pickup_any_item`). A mob without `shareables` wants nothing.
pub fn will_pickup(world: &World, mob: Entity, item: &ItemStack, any: bool) -> bool {
    let Some(wants) = world.get::<ShareablesConfig>(mob) else {
        return false;
    };
    let entry = wants.entry(&item.item_id, item.damage);
    if !any && !wants.all_items && entry.is_none() {
        return false;
    }
    let equipment = world.get::<MobEquipment>(mob);
    let max_amount = entry.map_or(wants.all_items_max_amount, |e| e.max_amount);
    let hand = carried(equipment, held_slot(&item.item_id));
    if let Some(slot) = worn_slot(&item.item_id) {
        let worn = carried(equipment, slot);
        if worn.is_empty() && wants.singular_pickup && hand.is_empty() {
            return true;
        }
        if has_binding_curse(&worn) {
            return false;
        }
        if same_item(&worn, item) && durability_damage(item) >= durability_damage(&worn) {
            return false;
        }
        return entry.is_some() && should_replace(&worn, item, wants, any);
    }
    if max_amount >= 0 && entry.is_some_and(|e| e.consume_item) {
        return true;
    }
    if hand_takes(item, &hand, max_amount, wants.singular_pickup) {
        return true;
    }
    if stackable(&hand, item) && durability_damage(item) >= durability_damage(&hand) {
        return false;
    }
    entry.is_some() && should_replace(&hand, item, wants, any)
}

/// `pickup_same_items_as_in_hand` (provisional: Microsoft Learn's
/// `behavior.pickup_items` page, "only pick up items that match what's in
/// their hand"): the preferred hand holds a stack `item` joins and that
/// stack is not full.
pub fn joins_hand(world: &World, mob: Entity, item: &ItemStack) -> bool {
    let hand = carried(world.get::<MobEquipment>(mob), held_slot(&item.item_id));
    stackable(&hand, item) && hand.count < hand.max_stack_size()
}

/// The `shareables` entry of `item` uses the item up on pickup.
pub fn consumes(world: &World, mob: Entity, item: &ItemStack) -> bool {
    world
        .get::<ShareablesConfig>(mob)
        .and_then(|wants| wants.entry(&item.item_id, item.damage))
        .is_some_and(|entry| entry.consume_item)
}

/// How many of the item actor stack `item` the mob takes: the whole stack,
/// limited by the `shareables` entry's `max_amount` (counting what the hand
/// already holds) and `pickup_limit`, then by the room left in a hand
/// stack it joins.
pub fn count_to_pickup(world: &World, mob: Entity, item: &ItemStack) -> u8 {
    let equipment = world.get::<MobEquipment>(mob);
    let hand = carried(equipment, held_slot(&item.item_id));
    let mut count = i32::from(item.count);
    if let Some(wants) = world.get::<ShareablesConfig>(mob) {
        let entry = wants.entry(&item.item_id, item.damage);
        let max_amount = entry.map_or(wants.all_items_max_amount, |e| e.max_amount);
        let pickup_limit = entry.map_or(-1, |e| e.pickup_limit);
        if max_amount >= 0 {
            let held = if same_item(&hand, item) {
                i32::from(hand.count)
            } else {
                0
            };
            count = count.min(max_amount - held);
        }
        if pickup_limit >= 0 && pickup_limit <= count {
            count = pickup_limit;
        }
    }
    if stackable(&hand, item) {
        count = count.min(i32::from(hand.max_stack_size()) - i32::from(hand.count));
    }
    count.clamp(0, i32::from(u8::MAX)) as u8
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::entity::mob::components::ItemMatcher;
    use crate::entity::mob::components::equipment::ShareableEntry;
    use crate::item::enchant::{with_durability_damage, with_enchantment_ids};

    fn entry(item: &str, priority: i32) -> ShareableEntry {
        ShareableEntry {
            item: ItemMatcher::parse(item).unwrap(),
            priority,
            max_amount: -1,
            pickup_limit: -1,
            want_amount: 1,
            surplus_amount: 1,
            consume_item: false,
        }
    }

    /// A zombie-like mob: swords and helmets ranked, `singular_pickup`.
    fn mob(world: &mut World) -> Entity {
        world
            .spawn((
                ShareablesConfig {
                    items: Arc::from(vec![
                        entry("minecraft:diamond_sword", 1),
                        entry("minecraft:iron_sword", 2),
                        entry("minecraft:stone_sword", 5),
                        entry("minecraft:iron_helmet", 2),
                        entry("minecraft:golden_helmet", 4),
                    ]),
                    all_items: false,
                    all_items_max_amount: -1,
                    all_items_want_amount: -1,
                    all_items_surplus_amount: -1,
                    singular_pickup: true,
                },
                MobEquipment::default(),
            ))
            .id()
    }

    fn hold(world: &mut World, mob: Entity, slot: GearSlot, stack: ItemStack) {
        world
            .get_mut::<MobEquipment>(mob)
            .unwrap()
            .set(slot, stack, false);
    }

    #[test]
    fn better_weapons_replace_worse_ones() {
        let mut world = World::new();
        let mob = mob(&mut world);
        let iron = ItemStack::new("minecraft:iron_sword", 1);
        let diamond = ItemStack::new("minecraft:diamond_sword", 1);
        let stone = ItemStack::new("minecraft:stone_sword", 1);
        let dirt = ItemStack::new("minecraft:dirt", 1);
        assert!(will_pickup(&world, mob, &iron, false));
        assert!(!will_pickup(&world, mob, &dirt, false));
        assert!(
            will_pickup(&world, mob, &dirt, true),
            "any item to an empty hand"
        );
        hold(&mut world, mob, GearSlot::Mainhand, iron.clone());
        assert!(will_pickup(&world, mob, &diamond, true));
        assert!(!will_pickup(&world, mob, &stone, true));
        assert!(!will_pickup(&world, mob, &dirt, true));
        assert!(
            !will_pickup(&world, mob, &iron, true),
            "same sword, no less worn"
        );
        hold(
            &mut world,
            mob,
            GearSlot::Mainhand,
            with_durability_damage(&iron, 100),
        );
        assert!(will_pickup(&world, mob, &iron, true), "a fresher copy");
    }

    #[test]
    fn armour_fills_empty_slots_and_upgrades_by_priority() {
        let mut world = World::new();
        let mob = mob(&mut world);
        let iron = ItemStack::new("minecraft:iron_helmet", 1);
        let gold = ItemStack::new("minecraft:golden_helmet", 1);
        let leather = ItemStack::new("minecraft:leather_helmet", 1);
        assert!(will_pickup(&world, mob, &gold, false));
        assert!(
            will_pickup(&world, mob, &leather, true),
            "empty slot, empty hand"
        );
        hold(&mut world, mob, GearSlot::Head, gold.clone());
        assert!(will_pickup(&world, mob, &iron, false));
        assert!(!will_pickup(&world, mob, &leather, true), "unlisted armour");
        hold(
            &mut world,
            mob,
            GearSlot::Head,
            with_enchantment_ids(&gold, [(BINDING_CURSE_ID, 1)]),
        );
        assert!(!will_pickup(&world, mob, &iron, false), "binding curse");
    }

    #[test]
    fn counts_stop_at_the_hand_stack_limit() {
        let mut world = World::new();
        let mob = mob(&mut world);
        let arrows = ItemStack::new("minecraft:arrow", 40).with_max_stack_size(64);
        assert_eq!(count_to_pickup(&world, mob, &arrows), 40);
        hold(
            &mut world,
            mob,
            GearSlot::Mainhand,
            ItemStack::new("minecraft:arrow", 30).with_max_stack_size(64),
        );
        assert_eq!(count_to_pickup(&world, mob, &arrows), 34);
        assert!(joins_hand(&world, mob, &arrows));
        assert!(!joins_hand(
            &world,
            mob,
            &ItemStack::new("minecraft:stick", 1)
        ));
    }
}
