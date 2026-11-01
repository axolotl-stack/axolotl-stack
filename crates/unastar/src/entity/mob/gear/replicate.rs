//! The equipment packets a mob's viewers receive.
//!
//! A new viewer gets, right after the mob's `AddActor`, the native
//! inventory send: `MobEquipment` for the main hand (slot 0, selected 0,
//! container 0) and the off hand (slot 1, selected 1, the off-hand
//! container), then one `MobArmorEquipment` with the four armour slots and
//! an empty body slot. A slot that changes later is sent on its own as the
//! equip goal sends it: a hand as `MobEquipment` (its slot, selected 0,
//! container 0), an armour slot as `MobEquipment` with slot `2..=5` and the
//! armour container.

use jolyne::valentine::types::{
    ActorRuntimeId, CerealizerNetworkItemStackDescriptorSerializedData as WireItem,
};
use jolyne::valentine::{McpePacket, MobArmorEquipmentPacket, MobEquipmentPacket};
use tracing::warn;

use super::{GearSlot, MobEquipment};
use crate::entity::mob::constants::{GEAR_ARMOUR_CONTAINER, GEAR_OFFHAND_CONTAINER};
use crate::item::ItemStack;
use crate::registry::ItemRegistry;
use crate::registry::block::BlockNetworkIds;
use crate::server::game::item_descriptor;

fn wire(stack: &ItemStack, items: &ItemRegistry, block_ids: BlockNetworkIds) -> WireItem {
    item_descriptor(stack, items, block_ids).unwrap_or_else(|| {
        warn!(item = %stack.item_id(), "Mob equipment item has no network form");
        WireItem::default()
    })
}

fn equipment_packet(
    runtime_id: i64,
    stack: &ItemStack,
    slot: u8,
    selected_slot: u8,
    container_id: u8,
    items: &ItemRegistry,
    block_ids: BlockNetworkIds,
) -> McpePacket {
    McpePacket::from(MobEquipmentPacket {
        target_runtime_id: ActorRuntimeId {
            actor_runtime_id: runtime_id as u64,
        },
        item: wire(stack, items, block_ids),
        slot,
        selected_slot,
        container_id,
    })
}

/// The native main-hand display update, independent of physical inventory.
pub(crate) fn mainhand_packet(
    runtime_id: i64,
    stack: &ItemStack,
    items: &ItemRegistry,
    block_ids: BlockNetworkIds,
) -> McpePacket {
    equipment_packet(runtime_id, stack, 0, 0, 0, items, block_ids)
}

/// The packets a new viewer of the mob receives after its `AddActor`.
pub fn inventory_packets(
    runtime_id: i64,
    equipment: &MobEquipment,
    items: &ItemRegistry,
    block_ids: BlockNetworkIds,
) -> [McpePacket; 3] {
    inventory_packets_projected(runtime_id, equipment, items, block_ids, None)
}

/// The full equipment view with an optional display-only main-hand stack.
pub(crate) fn inventory_packets_projected(
    runtime_id: i64,
    equipment: &MobEquipment,
    items: &ItemRegistry,
    block_ids: BlockNetworkIds,
    mainhand: Option<&ItemStack>,
) -> [McpePacket; 3] {
    let armour = |slot| wire(equipment.get(slot), items, block_ids);
    [
        equipment_packet(
            runtime_id,
            mainhand.unwrap_or_else(|| equipment.get(GearSlot::Mainhand)),
            GearSlot::Mainhand as u8,
            GearSlot::Mainhand as u8,
            0,
            items,
            block_ids,
        ),
        equipment_packet(
            runtime_id,
            equipment.get(GearSlot::Offhand),
            GearSlot::Offhand as u8,
            GearSlot::Offhand as u8,
            GEAR_OFFHAND_CONTAINER,
            items,
            block_ids,
        ),
        McpePacket::from(MobArmorEquipmentPacket {
            target_runtime_id: ActorRuntimeId {
                actor_runtime_id: runtime_id as u64,
            },
            head: armour(GearSlot::Head),
            torso: armour(GearSlot::Chest),
            legs: armour(GearSlot::Legs),
            feet: armour(GearSlot::Feet),
            body: WireItem::default(),
        }),
    ]
}

/// The packets for the slots changed since the last call, clearing the
/// change record.
pub fn changed_packets(
    runtime_id: i64,
    equipment: &mut MobEquipment,
    items: &ItemRegistry,
    block_ids: BlockNetworkIds,
    out: &mut Vec<McpePacket>,
) {
    changed_packets_projected(runtime_id, equipment, items, block_ids, out, None);
}

/// Equipment changes preserve an active display projection while physical
/// hand ownership continues to update independently.
pub(crate) fn changed_packets_projected(
    runtime_id: i64,
    equipment: &mut MobEquipment,
    items: &ItemRegistry,
    block_ids: BlockNetworkIds,
    out: &mut Vec<McpePacket>,
    mainhand: Option<&ItemStack>,
) {
    for slot in equipment.take_dirty() {
        let stack = if slot == GearSlot::Mainhand {
            mainhand.unwrap_or_else(|| equipment.get(slot))
        } else {
            equipment.get(slot)
        };
        out.push(if slot.is_hand() {
            equipment_packet(runtime_id, stack, slot as u8, 0, 0, items, block_ids)
        } else {
            equipment_packet(
                runtime_id,
                stack,
                slot as u8,
                slot as u8,
                GEAR_ARMOUR_CONTAINER,
                items,
                block_ids,
            )
        });
    }
}

#[cfg(test)]
mod tests {
    use jolyne::valentine::McpePacketData;

    use super::*;

    fn registry() -> ItemRegistry {
        let mut items = ItemRegistry::new();
        items.load_vanilla();
        items
    }

    #[test]
    fn new_viewers_get_hands_then_armour() {
        let items = registry();
        let mut equipment = MobEquipment::default();
        equipment.set(
            GearSlot::Mainhand,
            items.stack("minecraft:iron_sword", 1).unwrap(),
            false,
        );
        equipment.set(
            GearSlot::Feet,
            items.stack("minecraft:iron_boots", 1).unwrap(),
            false,
        );
        let [main, off, armour] = inventory_packets(7, &equipment, &items, BlockNetworkIds::Hashed);
        let sword = items
            .get_by_name("minecraft:iron_sword")
            .unwrap()
            .network_id();
        match main.data {
            McpePacketData::MobEquipmentPacket(p) => {
                assert_eq!(p.target_runtime_id.actor_runtime_id, 7);
                assert_eq!(i32::from(p.item.id), sword);
                assert_eq!((p.slot, p.selected_slot, p.container_id), (0, 0, 0));
            }
            other => panic!("{other:?}"),
        }
        match off.data {
            McpePacketData::MobEquipmentPacket(p) => {
                assert_eq!(p.item, WireItem::default());
                assert_eq!((p.slot, p.selected_slot, p.container_id), (1, 1, 119));
            }
            other => panic!("{other:?}"),
        }
        match armour.data {
            McpePacketData::MobArmorEquipmentPacket(p) => {
                assert_eq!(p.head, WireItem::default());
                assert_ne!(p.feet, WireItem::default());
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn changes_are_sent_per_slot_once() {
        let items = registry();
        let mut equipment = MobEquipment::default();
        equipment.set(
            GearSlot::Head,
            items.stack("minecraft:iron_helmet", 1).unwrap(),
            true,
        );
        let mut out = Vec::new();
        let ids = BlockNetworkIds::Hashed;
        changed_packets(3, &mut equipment, &items, ids, &mut out);
        changed_packets(3, &mut equipment, &items, ids, &mut out);
        assert_eq!(out.len(), 1);
        match &out[0].data {
            McpePacketData::MobEquipmentPacket(p) => {
                assert_eq!((p.slot, p.container_id), (2, 120));
            }
            other => panic!("{other:?}"),
        }
    }
}
