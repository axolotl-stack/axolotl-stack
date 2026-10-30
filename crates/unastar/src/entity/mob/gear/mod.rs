//! What a mob holds and wears.
//!
//! [`MobEquipment`] keeps the six native equipment slots (main hand, off
//! hand, head, chest, legs, feet) with the per-slot drop chance and whether
//! each item was picked up; [`worn_slot`] and [`held_slot`] classify items.
//!
//! - [`spawn`]: the `minecraft:equipment` table roll and the
//!   `pickup_based_on_chance` roll.
//! - [`choice`]: which ground items a mob wants (`minecraft:shareables`).
//! - [`drops`]: equipment dropped on death.
//! - [`replicate`]: the equipment packets viewers receive.
//! - [`persist`]: the equipment tags of the mob's saved NBT.
//!
//! The goals that pick items up and equip them are `mob::ai::pickup`.

pub mod choice;
pub mod drops;
pub mod persist;
pub mod replicate;
pub mod spawn;

use bevy_ecs::prelude::*;
use unastar_data::actor_components::body::EquipmentSlot;
use unastar_data::armour::ArmourSlot;

use crate::entity::mob::components::ItemMatcher;
use crate::entity::mob::constants::SHIELD_ITEM;
use crate::item::ItemStack;
use crate::registry::item::ItemEntry;

/// A native equipment slot, numbered as BDS numbers them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GearSlot {
    Mainhand = 0,
    Offhand = 1,
    Head = 2,
    Chest = 3,
    Legs = 4,
    Feet = 5,
}

impl GearSlot {
    pub const COUNT: usize = 6;
    pub const ALL: [GearSlot; Self::COUNT] = [
        GearSlot::Mainhand,
        GearSlot::Offhand,
        GearSlot::Head,
        GearSlot::Chest,
        GearSlot::Legs,
        GearSlot::Feet,
    ];
    /// The armour slots in their native order (head, chest, legs, feet).
    pub const ARMOUR: [GearSlot; 4] = [
        GearSlot::Head,
        GearSlot::Chest,
        GearSlot::Legs,
        GearSlot::Feet,
    ];

    /// The slot a behaviour pack names; `slot.armor.body` (horse and wolf
    /// armour) has no slot here.
    pub fn from_body(slot: EquipmentSlot) -> Option<Self> {
        Some(match slot {
            EquipmentSlot::SlotWeaponMainhand => Self::Mainhand,
            EquipmentSlot::SlotWeaponOffhand => Self::Offhand,
            EquipmentSlot::SlotArmorHead => Self::Head,
            EquipmentSlot::SlotArmorChest => Self::Chest,
            EquipmentSlot::SlotArmorLegs => Self::Legs,
            EquipmentSlot::SlotArmorFeet => Self::Feet,
            EquipmentSlot::SlotArmorBody => return None,
        })
    }

    fn from_armour(slot: ArmourSlot) -> Self {
        match slot {
            ArmourSlot::Head => Self::Head,
            ArmourSlot::Chest => Self::Chest,
            ArmourSlot::Legs => Self::Legs,
            ArmourSlot::Feet => Self::Feet,
        }
    }

    pub fn is_hand(self) -> bool {
        matches!(self, Self::Mainhand | Self::Offhand)
    }

    /// The slot's name in saved NBT (`SlotDropChances`).
    pub fn name(self) -> &'static str {
        match self {
            Self::Mainhand => "mainhand",
            Self::Offhand => "offhand",
            Self::Head => "head",
            Self::Chest => "chest",
            Self::Legs => "legs",
            Self::Feet => "feet",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|slot| slot.name() == name)
    }

    fn bit(self) -> u8 {
        1 << self as u8
    }
}

/// The armour slot `item` is worn in: armour by its measured slot, and
/// head-wearable blocks (carved pumpkin, skulls and heads) on the head.
pub fn worn_slot(item: &ItemEntry) -> Option<GearSlot> {
    if let Some(slot) = unastar_data::armour::armour_slot(item.identifier()) {
        return Some(GearSlot::from_armour(slot));
    }
    item.data().head.then_some(GearSlot::Head)
}

/// The hand a picked-up `item_id` goes to: the shield to the off hand,
/// everything else to the main hand.
pub fn held_slot(item_id: &str) -> GearSlot {
    if item_id == SHIELD_ITEM {
        GearSlot::Offhand
    } else {
        GearSlot::Mainhand
    }
}

/// One equipped item.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GearItem {
    pub stack: ItemStack,
    /// Taken from the ground: always dropped on death.
    pub picked_up: bool,
}

/// The mob's equipment. Present on every mob with an equipment component
/// (inserted when the gear table is rolled or an item is first equipped).
#[derive(Component, Debug, Clone, Default)]
pub struct MobEquipment {
    items: [GearItem; GearSlot::COUNT],
    /// Authored drop chance per slot; `None` uses the native default.
    drop_chances: [Option<f32>; GearSlot::COUNT],
    /// Slots changed since viewers were last told, one bit per slot
    /// ([`replicate`]).
    dirty: u8,
}

impl MobEquipment {
    pub fn get(&self, slot: GearSlot) -> &ItemStack {
        &self.items[slot as usize].stack
    }

    pub fn item(&self, slot: GearSlot) -> &GearItem {
        &self.items[slot as usize]
    }

    /// Put `stack` in `slot`, returning what was there.
    pub fn set(&mut self, slot: GearSlot, stack: ItemStack, picked_up: bool) -> GearItem {
        self.dirty |= slot.bit();
        let item = if stack.is_empty() {
            GearItem::default()
        } else {
            GearItem { stack, picked_up }
        };
        std::mem::replace(&mut self.items[slot as usize], item)
    }

    /// Empty `slot`, returning what was there.
    pub fn take(&mut self, slot: GearSlot) -> GearItem {
        self.set(slot, ItemStack::empty(), false)
    }

    pub fn drop_chance(&self, slot: GearSlot) -> Option<f32> {
        self.drop_chances[slot as usize]
    }

    pub fn set_drop_chance(&mut self, slot: GearSlot, chance: Option<f32>) {
        self.drop_chances[slot as usize] = chance;
    }

    /// Number of non-empty slots.
    pub fn count(&self) -> u32 {
        self.items.iter().filter(|i| !i.stack.is_empty()).count() as u32
    }

    /// Whether a slot changed since [`Self::take_dirty`] last ran.
    pub fn is_dirty(&self) -> bool {
        self.dirty != 0
    }

    /// The slots changed since the last call, clearing the record.
    pub(crate) fn take_dirty(&mut self) -> impl Iterator<Item = GearSlot> + use<> {
        let dirty = std::mem::take(&mut self.dirty);
        GearSlot::ALL
            .into_iter()
            .filter(move |slot| dirty & slot.bit() != 0)
    }
}

/// The spawn-time roll of `behavior.pickup_items` `pickup_based_on_chance`:
/// whether this mob may pick items up at all (saved as `canPickupItems`).
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct CanPickUpLoot(pub bool);

/// An item taken from the ground waiting for `behavior.equip_item` (the
/// native equip-item pending stack). Removed once equipped.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct PendingEquip(pub ItemStack);

/// The item on the mob's head, if any.
pub fn helmet(world: &World, mob: Entity) -> Option<&ItemStack> {
    world
        .get::<MobEquipment>(mob)
        .map(|equipment| equipment.get(GearSlot::Head))
        .filter(|stack| !stack.is_empty())
}

/// Number of equipped items (`query.equipment_count`).
pub fn equipment_count(world: &World, mob: Entity) -> u32 {
    world
        .get::<MobEquipment>(mob)
        .map_or(0, MobEquipment::count)
}

/// The non-empty stack in the mob's main hand.
pub fn mainhand(world: &World, mob: Entity) -> Option<&ItemStack> {
    world
        .get::<MobEquipment>(mob)
        .map(|equipment| equipment.get(GearSlot::Mainhand))
        .filter(|stack| !stack.is_empty())
}

/// The slots a `has_equipment` filter `domain` names; `None` for a domain
/// that is not an equipment domain. `body` and `inventory` name no slot a
/// supported mob has.
pub fn domain_slots(domain: &str) -> Option<&'static [GearSlot]> {
    Some(match domain {
        "any" => &GearSlot::ALL,
        "hand" => &[GearSlot::Mainhand, GearSlot::Offhand],
        "armor" => &GearSlot::ARMOUR,
        "head" => &[GearSlot::Head],
        "torso" => &[GearSlot::Chest],
        "leg" => &[GearSlot::Legs],
        "feet" => &[GearSlot::Feet],
        "body" | "inventory" => &[],
        _ => return None,
    })
}

/// The `has_equipment` test on a mob: whether an item `want` matches is in
/// one of `slots`.
pub fn has_equipment(world: &World, mob: Entity, slots: &[GearSlot], want: &ItemMatcher) -> bool {
    world.get::<MobEquipment>(mob).is_some_and(|equipment| {
        slots.iter().any(|&slot| {
            let stack = equipment.get(slot);
            !stack.is_empty() && want.matches(stack.item_id(), stack.damage)
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::item::test_support::{VANILLA, stack};

    fn worn(item: &str) -> Option<GearSlot> {
        worn_slot(VANILLA.get_by_name(item).expect(item))
    }

    #[test]
    fn slots_follow_measured_armour_and_head_wearables() {
        assert_eq!(worn("minecraft:iron_helmet"), Some(GearSlot::Head));
        assert_eq!(worn("minecraft:elytra"), Some(GearSlot::Chest));
        assert_eq!(worn("minecraft:diamond_leggings"), Some(GearSlot::Legs));
        assert_eq!(worn("minecraft:golden_boots"), Some(GearSlot::Feet));
        assert_eq!(worn("minecraft:turtle_helmet"), Some(GearSlot::Head));
        assert_eq!(worn("minecraft:carved_pumpkin"), Some(GearSlot::Head));
        assert_eq!(worn("minecraft:iron_sword"), None);
        assert_eq!(held_slot("minecraft:shield"), GearSlot::Offhand);
        assert_eq!(held_slot("minecraft:iron_sword"), GearSlot::Mainhand);
    }

    #[test]
    fn changed_slots_are_reported_once() {
        let mut equipment = MobEquipment::default();
        assert!(!equipment.is_dirty());
        equipment.set(
            GearSlot::Chest,
            stack("minecraft:iron_chestplate", 1),
            false,
        );
        equipment.set(GearSlot::Mainhand, stack("minecraft:iron_sword", 1), true);
        assert_eq!(
            equipment.take_dirty().collect::<Vec<_>>(),
            [GearSlot::Mainhand, GearSlot::Chest]
        );
        assert!(!equipment.is_dirty());
        assert_eq!(equipment.count(), 2);
        assert!(equipment.item(GearSlot::Mainhand).picked_up);
        let old = equipment.take(GearSlot::Mainhand);
        assert_eq!(old.stack.item_id(), "minecraft:iron_sword");
        assert_eq!(equipment.count(), 1);
        assert_eq!(
            equipment.take_dirty().collect::<Vec<_>>(),
            [GearSlot::Mainhand]
        );
    }

    #[test]
    fn has_equipment_domains_name_slots() {
        assert_eq!(domain_slots("hand").unwrap().len(), 2);
        assert_eq!(domain_slots("armor").unwrap(), &GearSlot::ARMOUR);
        assert_eq!(domain_slots("torso").unwrap(), &[GearSlot::Chest]);
        assert!(domain_slots("inventory").unwrap().is_empty());
        assert!(domain_slots("tail").is_none());
        for slot in GearSlot::ALL {
            assert_eq!(GearSlot::from_name(slot.name()), Some(slot));
        }
    }
}
