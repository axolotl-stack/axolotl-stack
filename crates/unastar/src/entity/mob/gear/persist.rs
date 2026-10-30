//! The equipment tags of a mob's saved NBT (the table is in the
//! `mob::persistence` module docs).
//!
//! `Mainhand` and `Offhand` are lists of one item compound, `Armor` a list
//! of four (head, chest, legs, feet); an empty slot is an item compound
//! with an empty `Name` and `Count` 0. An item compound carries `Name`
//! (String), `Count` (Byte), `Damage` (Short, the data value), `WasPickedUp`
//! (Byte) and, when the item has one, its `tag` compound (enchantments,
//! wear). `SlotDropChances` lists the slots with a recorded drop chance as
//! `{Slot: String, DropChance: Float}`; `canPickupItems` and
//! `hasSetCanPickupItems` (Bytes) keep the `pickup_based_on_chance` roll.

use std::collections::HashMap;

use bevy_ecs::prelude::*;
use zuri_nbt::{NBTTag, tag};

use super::{CanPickUpLoot, GearItem, GearSlot, MobEquipment, spawn::roll_loot_pickup};
use crate::entity::mob::components::PickupItemsGoal;
use crate::item::ItemStack;
use crate::item::enchant::{item_tag, with_item_tag};
use crate::server::game::world_stack;

/// Why the saved equipment of a mob cannot be restored.
#[derive(Debug, thiserror::Error)]
pub enum GearDecodeError {
    #[error("equipment tag {name} is not {kind}")]
    WrongType {
        name: &'static str,
        kind: &'static str,
    },
    #[error("equipment list {name} has {len} items, not {expected}")]
    WrongLength {
        name: &'static str,
        len: usize,
        expected: usize,
    },
    #[error("SlotDropChances names unknown slot {0:?}")]
    UnknownSlot(String),
    #[error("equipped item {0} is not in the item registry")]
    UnknownItem(String),
}

fn list(values: impl IntoIterator<Item = NBTTag>) -> NBTTag {
    NBTTag::List(tag::List(values.into_iter().collect()))
}

fn byte(value: bool) -> NBTTag {
    NBTTag::Byte(tag::Byte(u8::from(value)))
}

fn item_nbt(item: &GearItem) -> NBTTag {
    let stack = &item.stack;
    if stack.is_empty() {
        return NBTTag::Compound(
            tag::Compound::builder()
                .with_string("Name", "")
                .with_byte("Count", 0u8)
                .with_short("Damage", 0i16)
                .with_byte("WasPickedUp", 0u8)
                .build(),
        );
    }
    let mut compound = tag::Compound::builder()
        .with_string("Name", stack.item_id())
        .with_byte("Count", stack.count)
        .with_short("Damage", stack.damage)
        .with_byte("WasPickedUp", u8::from(item.picked_up))
        .build();
    if let Some(item_tag) = item_tag(stack) {
        compound
            .0
            .insert("tag".to_owned(), NBTTag::Compound(item_tag));
    }
    NBTTag::Compound(compound)
}

/// The equipment tags of `mob`, empty for a mob without equipment.
pub fn save(world: &World, mob: Entity) -> Vec<(&'static str, NBTTag)> {
    let mut out = Vec::new();
    if let Some(equipment) = world.get::<MobEquipment>(mob) {
        let item = |slot| item_nbt(equipment.item(slot));
        out.push(("Mainhand", list([item(GearSlot::Mainhand)])));
        out.push(("Offhand", list([item(GearSlot::Offhand)])));
        out.push(("Armor", list(GearSlot::ARMOUR.map(item))));
        let chances: Vec<NBTTag> = GearSlot::ALL
            .into_iter()
            .filter_map(|slot| {
                let chance = equipment.drop_chance(slot)?;
                Some(NBTTag::Compound(
                    tag::Compound::builder()
                        .with_string("Slot", slot.name())
                        .with_float("DropChance", chance)
                        .build(),
                ))
            })
            .collect();
        if !chances.is_empty() {
            out.push(("SlotDropChances", list(chances)));
        }
    }
    if let Some(CanPickUpLoot(can)) = world.get::<CanPickUpLoot>(mob) {
        out.push(("canPickupItems", byte(*can)));
        out.push(("hasSetCanPickupItems", byte(true)));
    }
    out
}

fn compound<'a>(
    tag: &'a NBTTag,
    name: &'static str,
) -> Result<&'a HashMap<String, NBTTag>, GearDecodeError> {
    match tag {
        NBTTag::Compound(compound) => Ok(&compound.0),
        _ => Err(GearDecodeError::WrongType {
            name,
            kind: "an item compound",
        }),
    }
}

fn read_item(
    world: &World,
    tag: &NBTTag,
    name: &'static str,
) -> Result<(ItemStack, bool), GearDecodeError> {
    let fields = compound(tag, name)?;
    let wrong = |kind| GearDecodeError::WrongType { name, kind };
    let item_id = match fields.get("Name") {
        Some(NBTTag::String(value)) => value.0.as_str(),
        None => "",
        Some(_) => return Err(wrong("an item with a String Name")),
    };
    let count = match fields.get("Count") {
        Some(NBTTag::Byte(value)) => value.0,
        None => 0,
        Some(_) => return Err(wrong("an item with a Byte Count")),
    };
    if item_id.is_empty() || count == 0 {
        return Ok((ItemStack::empty(), false));
    }
    let damage = match fields.get("Damage") {
        Some(NBTTag::Short(value)) => value.0,
        None => 0,
        Some(_) => return Err(wrong("an item with a Short Damage")),
    };
    let picked_up = match fields.get("WasPickedUp") {
        Some(NBTTag::Byte(value)) => value.0 != 0,
        None => false,
        Some(_) => return Err(wrong("an item with a Byte WasPickedUp")),
    };
    let mut stack = world_stack(world, item_id, count)
        .ok_or_else(|| GearDecodeError::UnknownItem(item_id.to_owned()))?;
    stack.damage = damage;
    match fields.get("tag") {
        None => {}
        Some(NBTTag::Compound(item_tag)) => stack = with_item_tag(&stack, item_tag.clone()),
        Some(_) => return Err(wrong("an item with a compound tag")),
    }
    Ok((stack, picked_up))
}

fn items<'a>(
    fields: &'a HashMap<String, NBTTag>,
    name: &'static str,
    expected: usize,
) -> Result<Option<&'a [NBTTag]>, GearDecodeError> {
    match fields.get(name) {
        None => Ok(None),
        Some(NBTTag::List(list)) if list.0.len() == expected => Ok(Some(&list.0)),
        Some(NBTTag::List(list)) => Err(GearDecodeError::WrongLength {
            name,
            len: list.0.len(),
            expected,
        }),
        Some(_) => Err(GearDecodeError::WrongType {
            name,
            kind: "a list",
        }),
    }
}

/// Restore the saved equipment of `mob` from its actor compound `fields`.
/// The composition is already re-applied, so authored drop chances are in
/// place; saved ones override them.
pub fn restore(
    world: &mut World,
    mob: Entity,
    fields: &HashMap<String, NBTTag>,
) -> Result<(), GearDecodeError> {
    let mut saved = Vec::new();
    for (name, slots) in [
        ("Mainhand", &[GearSlot::Mainhand][..]),
        ("Offhand", &[GearSlot::Offhand][..]),
        ("Armor", &GearSlot::ARMOUR[..]),
    ] {
        if let Some(list) = items(fields, name, slots.len())? {
            for (slot, tag) in slots.iter().zip(list) {
                saved.push((*slot, read_item(world, tag, name)?));
            }
        }
    }
    let mut chances = Vec::new();
    match fields.get("SlotDropChances") {
        None => {}
        Some(NBTTag::List(list)) => {
            for entry in &list.0 {
                let entry = compound(entry, "SlotDropChances")?;
                let wrong = || GearDecodeError::WrongType {
                    name: "SlotDropChances",
                    kind: "a list of {Slot: String, DropChance: Float}",
                };
                let (Some(NBTTag::String(slot)), Some(NBTTag::Float(chance))) =
                    (entry.get("Slot"), entry.get("DropChance"))
                else {
                    return Err(wrong());
                };
                let slot = GearSlot::from_name(&slot.0)
                    .ok_or_else(|| GearDecodeError::UnknownSlot(slot.0.clone()))?;
                chances.push((slot, chance.0));
            }
        }
        Some(_) => {
            return Err(GearDecodeError::WrongType {
                name: "SlotDropChances",
                kind: "a list",
            });
        }
    }
    let flag = |name: &'static str| match fields.get(name) {
        None => Ok(false),
        Some(NBTTag::Byte(value)) => Ok(value.0 != 0),
        Some(_) => Err(GearDecodeError::WrongType {
            name,
            kind: "a Byte",
        }),
    };
    let can_pickup = flag("canPickupItems")?;
    let has_set = flag("hasSetCanPickupItems")?;

    let has_equipment = world.get::<MobEquipment>(mob).is_some();
    if has_equipment || !chances.is_empty() || saved.iter().any(|(_, (s, _))| !s.is_empty()) {
        let mut entity = world.entity_mut(mob);
        let mut equipment = entity.take::<MobEquipment>().unwrap_or_default();
        for (slot, (stack, picked_up)) in saved {
            equipment.set(slot, stack, picked_up);
        }
        for (slot, chance) in chances {
            equipment.set_drop_chance(slot, Some(chance));
        }
        entity.insert(equipment);
    }
    if has_set {
        world.entity_mut(mob).insert(CanPickUpLoot(can_pickup));
    } else if world
        .get::<PickupItemsGoal>(mob)
        .is_some_and(|goal| goal.pickup_based_on_chance)
    {
        // A record without the roll rolls it now, as a reload does.
        roll_loot_pickup(world, mob);
    }
    Ok(())
}
