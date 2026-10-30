//! Registry-compiled equipment permissions; placement reads typed fields only.
//!
//! Built-in armour slots/defense come from the BDS `armor_values` family.
//! `ItemData::head` and `allow_offhand` come from protocolgen's supplemental
//! Allay capability table (source lock revision
//! `59d4007e322a0acf2b0add59133c81e1f7e4c501`) and reviewed corrections,
//! not from the permissive `/replaceitem` head/offhand sweep.
//!
//! Custom component meanings follow the official component references:
//! <https://learn.microsoft.com/en-us/minecraft/creator/reference/content/itemreference/examples/itemcomponents/minecraft_wearable>
//! and <https://learn.microsoft.com/en-us/minecraft/creator/reference/content/itemreference/examples/itemcomponents/minecraft_allow_off_hand>.
//! Neither these references nor the local sweeps establish precedence when an
//! addon contradicts built-in facts. Such conflicts fail registry compilation
//! rather than claiming an unmeasured override rule. Body and mainhand wearable
//! kinds are retained, but are not mapped into the four player armour slots.

use unastar_data::equipment::{self, ArmourSlot};
use unastar_data::item_components::{ItemComponents, Wearable};
use unastar_data::items::ItemData;

use super::ItemStack;

/// Every documented wearable slot, including kinds outside player armour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WearableSlot {
    Armour(ArmourSlot),
    Body,
    Mainhand,
    Offhand,
}

/// Immutable equipment facts stored on each frozen registry entry.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct EquipmentPermissions {
    pub armour_slot: Option<ArmourSlot>,
    /// Protection from the measured row or typed wearable component, else zero.
    pub defense: f64,
    /// Preserve non-player slot kinds instead of silently treating them as head.
    pub wearable_slot: Option<WearableSlot>,
    pub allow_offhand: bool,
}

/// Unsupported or contradictory startup equipment facts.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum EquipmentError {
    #[error("item {identifier}: unsupported minecraft:wearable slot {slot:?}")]
    UnknownWearableSlot { identifier: String, slot: String },
    #[error(
        "item {identifier}: measured armour slot {measured:?} conflicts with native head capability"
    )]
    NativeSlotConflict {
        identifier: String,
        measured: ArmourSlot,
    },
    #[error(
        "item {identifier}: wearable slot {wearable:?} conflicts with native slot {native:?}; addon override precedence is unmeasured"
    )]
    SlotConflict {
        identifier: String,
        native: ArmourSlot,
        wearable: WearableSlot,
    },
    #[error(
        "item {identifier}: wearable protection {component} conflicts with measured defense {measured}; addon override precedence is unmeasured"
    )]
    DefenseConflict {
        identifier: String,
        measured: f64,
        component: i32,
    },
    #[error(
        "item {identifier}: minecraft:allow_off_hand=false conflicts with {source_kind}; addon override precedence is unmeasured"
    )]
    OffhandConflict {
        identifier: String,
        source_kind: &'static str,
    },
}

fn wearable_slot(data: &ItemData, wearable: &Wearable) -> Result<WearableSlot, EquipmentError> {
    Ok(match wearable.slot.as_str() {
        "slot.armor.head" => WearableSlot::Armour(ArmourSlot::Head),
        "slot.armor.chest" => WearableSlot::Armour(ArmourSlot::Chest),
        "slot.armor.legs" => WearableSlot::Armour(ArmourSlot::Legs),
        "slot.armor.feet" => WearableSlot::Armour(ArmourSlot::Feet),
        "slot.armor.body" => WearableSlot::Body,
        "slot.weapon.mainhand" => WearableSlot::Mainhand,
        "slot.weapon.offhand" => WearableSlot::Offhand,
        _ => {
            return Err(EquipmentError::UnknownWearableSlot {
                identifier: data.identifier.clone(),
                slot: wearable.slot.clone(),
            });
        }
    })
}

/// Compile once before freezing an item entry. Identifier lookup and component
/// slot strings are confined to this function, never placement/tick handling.
pub fn compile(
    data: &ItemData,
    components: &ItemComponents,
) -> Result<EquipmentPermissions, EquipmentError> {
    let measured = equipment::get(&data.identifier);
    let native_slot = measured.map(|row| row.slot);
    if let Some(measured) = native_slot
        && data.head
        && measured != ArmourSlot::Head
    {
        return Err(EquipmentError::NativeSlotConflict {
            identifier: data.identifier.clone(),
            measured,
        });
    }
    let native_slot = native_slot.or(data.head.then_some(ArmourSlot::Head));
    let wearable_slot = components
        .wearable
        .as_ref()
        .map(|wearable| wearable_slot(data, wearable))
        .transpose()?;
    if let (Some(native), Some(wearable)) = (native_slot, wearable_slot)
        && wearable != WearableSlot::Armour(native)
    {
        return Err(EquipmentError::SlotConflict {
            identifier: data.identifier.clone(),
            native,
            wearable,
        });
    }
    if let (Some(measured), Some(wearable)) = (measured, &components.wearable)
        && measured.defense != f64::from(wearable.protection)
    {
        return Err(EquipmentError::DefenseConflict {
            identifier: data.identifier.clone(),
            measured: measured.defense,
            component: wearable.protection,
        });
    }
    if components.allow_off_hand == Some(false) {
        let source_kind = if data.allow_offhand {
            Some("native offhand capability")
        } else if wearable_slot == Some(WearableSlot::Offhand) {
            Some("minecraft:wearable offhand slot")
        } else {
            None
        };
        if let Some(source_kind) = source_kind {
            return Err(EquipmentError::OffhandConflict {
                identifier: data.identifier.clone(),
                source_kind,
            });
        }
    }
    let component_armour = match wearable_slot {
        Some(WearableSlot::Armour(slot)) => Some(slot),
        _ => None,
    };
    Ok(EquipmentPermissions {
        armour_slot: native_slot.or(component_armour),
        defense: measured
            .map(|row| row.defense)
            .or_else(|| {
                components
                    .wearable
                    .as_ref()
                    .map(|wearable| f64::from(wearable.protection))
            })
            .unwrap_or(0.0),
        wearable_slot,
        allow_offhand: data.allow_offhand
            || components.allow_off_hand == Some(true)
            || wearable_slot == Some(WearableSlot::Offhand),
    })
}

/// Player armour slots: 0 head, 1 chest, 2 legs, 3 feet. Empty removes an item;
/// invalid indices are rejected even for an empty stack.
#[inline]
pub fn accepts_armour(stack: &ItemStack, slot: usize) -> bool {
    let Some(slot) = ArmourSlot::from_player_slot(slot) else {
        return false;
    };
    stack.is_empty()
        || stack
            .item()
            .is_some_and(|item| item.equipment.armour_slot == Some(slot))
}

/// Empty is valid removal; nonempty items need their compiled offhand permission.
#[inline]
pub fn allows_offhand(stack: &ItemStack) -> bool {
    stack.is_empty()
        || stack
            .item()
            .is_some_and(|item| item.equipment.allow_offhand)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::item::test_support::vanilla_row;
    use crate::registry::item::{ItemEntry, ItemRegistry};

    fn custom_data() -> ItemData {
        let mut data = vanilla_row("minecraft:stone");
        data.identifier = "addon:plain".to_owned();
        data.head = false;
        data.allow_offhand = false;
        data
    }

    fn stack(data: ItemData, components: ItemComponents) -> ItemStack {
        let name = data.identifier.clone();
        let mut registry = ItemRegistry::new();
        registry
            .register(ItemEntry::new(0, data, components).expect("valid equipment"))
            .unwrap();
        registry.stack(&name, 1).unwrap()
    }

    fn wearable(slot: &str, protection: i32) -> ItemComponents {
        ItemComponents {
            wearable: Some(Wearable {
                slot: slot.to_owned(),
                protection,
            }),
            ..ItemComponents::default()
        }
    }

    #[test]
    fn custom_typed_wearables_accept_exactly_their_player_slot() {
        for (name, expected) in [
            ("slot.armor.head", 0),
            ("slot.armor.chest", 1),
            ("slot.armor.legs", 2),
            ("slot.armor.feet", 3),
        ] {
            let stack = stack(custom_data(), wearable(name, 7));
            assert_eq!(stack.item().unwrap().equipment.defense, 7.0);
            for slot in 0..4 {
                assert_eq!(accepts_armour(&stack, slot), slot == expected);
            }
            assert!(!accepts_armour(&stack, 4));
            assert!(!accepts_armour(&stack, usize::MAX));
            assert!(!allows_offhand(&stack));
        }
    }

    #[test]
    fn measured_armour_rejects_mismatched_slots_and_ordinary_items() {
        let chest = stack(
            vanilla_row("minecraft:iron_chestplate"),
            ItemComponents::default(),
        );
        assert!(accepts_armour(&chest, 1));
        for slot in [0, 2, 3, 4, usize::MAX] {
            assert!(!accepts_armour(&chest, slot));
        }
        let ordinary = stack(vanilla_row("minecraft:stone"), ItemComponents::default());
        for slot in 0..4 {
            assert!(!accepts_armour(&ordinary, slot));
        }
        assert!(!allows_offhand(&ordinary));
    }

    #[test]
    fn legitimate_non_armour_head_capability_is_not_command_permissiveness() {
        let data = vanilla_row("minecraft:carved_pumpkin");
        assert!(data.head);
        assert!(equipment::get(&data.identifier).is_none());
        let pumpkin = stack(data, ItemComponents::default());
        assert!(accepts_armour(&pumpkin, 0));
        assert!(!accepts_armour(&pumpkin, 1));
        assert!(!allows_offhand(&pumpkin));
    }

    #[test]
    fn offhand_uses_native_and_typed_custom_capabilities() {
        let native = stack(vanilla_row("minecraft:arrow"), ItemComponents::default());
        assert!(allows_offhand(&native));
        assert!(!accepts_armour(&native, 0));
        for enabled in [false, true] {
            let custom = stack(
                custom_data(),
                ItemComponents {
                    allow_off_hand: Some(enabled),
                    ..ItemComponents::default()
                },
            );
            assert_eq!(allows_offhand(&custom), enabled);
            assert!(!accepts_armour(&custom, 0));
        }
        let custom = stack(custom_data(), wearable("slot.weapon.offhand", 0));
        assert!(allows_offhand(&custom));
        for slot in 0..4 {
            assert!(!accepts_armour(&custom, slot));
        }
    }

    #[test]
    fn empty_removal_respects_armour_slot_boundaries() {
        let empty = ItemStack::empty();
        for slot in 0..4 {
            assert!(accepts_armour(&empty, slot));
        }
        assert!(!accepts_armour(&empty, 4));
        assert!(!accepts_armour(&empty, usize::MAX));
        assert!(allows_offhand(&empty));
        let zero_count = stack(custom_data(), ItemComponents::default()).with_count(0);
        assert!(accepts_armour(&zero_count, 3));
        assert!(!accepts_armour(&zero_count, 4));
        assert!(allows_offhand(&zero_count));
    }

    #[test]
    fn non_player_kinds_are_retained_and_unknown_slots_fail() {
        for (name, kind) in [
            ("slot.armor.body", WearableSlot::Body),
            ("slot.weapon.mainhand", WearableSlot::Mainhand),
        ] {
            let custom = stack(custom_data(), wearable(name, 2));
            assert_eq!(custom.item().unwrap().equipment.wearable_slot, Some(kind));
            for slot in 0..4 {
                assert!(!accepts_armour(&custom, slot));
            }
            assert!(!allows_offhand(&custom));
        }
        assert!(matches!(
            compile(&custom_data(), &wearable("slot.armor.typo", 0)),
            Err(EquipmentError::UnknownWearableSlot { .. })
        ));
    }

    #[test]
    fn contradictory_sources_do_not_invent_addon_override_precedence() {
        let native = &vanilla_row("minecraft:iron_chestplate");
        assert!(matches!(
            compile(native, &wearable("slot.armor.feet", 6)),
            Err(EquipmentError::SlotConflict { .. })
        ));
        assert!(matches!(
            compile(native, &wearable("slot.armor.chest", 2)),
            Err(EquipmentError::DefenseConflict { .. })
        ));
        let matching = compile(native, &wearable("slot.armor.chest", 6)).unwrap();
        assert_eq!(matching.armour_slot, Some(ArmourSlot::Chest));
        assert_eq!(matching.defense, 6.0);
        let mut contradictory_native = native.clone();
        contradictory_native.head = true;
        assert!(matches!(
            compile(&contradictory_native, &ItemComponents::default()),
            Err(EquipmentError::NativeSlotConflict {
                measured: ArmourSlot::Chest,
                ..
            })
        ));
        let mut head_capability = custom_data();
        head_capability.head = true;
        assert!(matches!(
            compile(&head_capability, &wearable("slot.armor.legs", 0)),
            Err(EquipmentError::SlotConflict {
                native: ArmourSlot::Head,
                wearable: WearableSlot::Armour(ArmourSlot::Legs),
                ..
            })
        ));
        let no_offhand = ItemComponents {
            allow_off_hand: Some(false),
            ..ItemComponents::default()
        };
        let native = &vanilla_row("minecraft:arrow");
        assert!(matches!(
            compile(native, &no_offhand),
            Err(EquipmentError::OffhandConflict { .. })
        ));
        let mut contradictory = wearable("slot.weapon.offhand", 0);
        contradictory.allow_off_hand = Some(false);
        assert!(matches!(
            compile(&custom_data(), &contradictory),
            Err(EquipmentError::OffhandConflict { .. })
        ));
    }
}
