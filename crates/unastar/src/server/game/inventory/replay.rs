//! Shared pieces of the tests that replay BDS captures through the request
//! engine: stack-ID pairing, window seeding and response comparison.

use std::collections::HashMap;

use jolyne::valentine::{
    CerealizerNetworkItemStackDescriptorSerializedData as WireItem,
    ItemStackRequestCerealSlotInfoData, ItemStackRequestPacketDataRequestData,
    ItemStackRequestPacketDataRequestDataActionsItem as Action, ItemStackResponseInfo,
};

use super::engine::Stores;
use crate::item::ItemStack;
use crate::item::test_support::VANILLA;

/// Pairs BDS stack network IDs with ours: the replay must reuse an ID
/// exactly where BDS does and mint a fresh one exactly where BDS does.
#[derive(Default)]
pub struct IdMap {
    ours: HashMap<i32, i32>,
    theirs: HashMap<i32, i32>,
}

impl IdMap {
    pub fn pair(&mut self, bds: i32, ours: i32, context: &str) {
        match (self.ours.get(&bds), self.theirs.get(&ours)) {
            (Some(&o), _) => assert_eq!(o, ours, "{context}: BDS reused {bds}"),
            (None, Some(&b)) => {
                panic!("{context}: we reused {ours} where BDS minted {bds} (was {b})")
            }
            (None, None) => {
                self.ours.insert(bds, ours);
                self.theirs.insert(ours, bds);
            }
        }
    }

    pub fn to_ours(&self, bds: i32) -> i32 {
        if bds <= 0 {
            bds
        } else {
            // An ID BDS never assigned (the stale-ID step) stays unknown.
            self.ours.get(&bds).copied().unwrap_or(i32::MAX)
        }
    }

    /// Our ID for a BDS ID already paired.
    pub fn known(&self, bds: i32) -> Option<i32> {
        self.ours.get(&bds).copied()
    }
}

pub fn stack_of(wire: &WireItem, ids: &mut IdMap) -> ItemStack {
    if wire.id == 0 {
        return ItemStack::empty();
    }
    let item = VANILLA.item_type_by_network_id(i32::from(wire.id)).unwrap();
    let bds = wire.net_id_variant.unwrap();
    // A resent stack keeps the ID it already has.
    let known = ids.ours.get(&bds).copied().unwrap_or(0);
    let stack = ItemStack::new(item, wire.stacksize as u8)
        .with_damage(wire.auxvalue as i16)
        .with_runtime_id(wire.block_runtime_id as i32)
        .with_nbt(wire.user_data_buffer.clone())
        .with_net_id(known)
        .into_slot();
    ids.pair(bds, stack.net_id, "InventoryContent");
    stack
}

pub fn set_window(container: u32, slots: &[WireItem], stores: &mut Stores<'_>, ids: &mut IdMap) {
    for (index, wire) in slots.iter().enumerate() {
        set_slot(container, index, wire, stores, ids);
    }
}

pub fn set_slot(
    container: u32,
    index: usize,
    wire: &WireItem,
    stores: &mut Stores<'_>,
    ids: &mut IdMap,
) {
    let stack = stack_of(wire, ids);
    match container {
        0 => drop(stores.main.set_item(index, stack)),
        119 => *stores.offhand = stack,
        120 if index < 4 => drop(stores.armor.set_slot(index, stack)),
        124 => drop(stores.ui.set_item(index, stack)),
        _ => {}
    }
}

/// Rewrite the request's BDS stack IDs to ours (the grindstone names the
/// input stack in its recipe field).
pub fn translate(request: &mut ItemStackRequestPacketDataRequestData, ids: &IdMap) {
    let fix = |info: &mut ItemStackRequestCerealSlotInfoData| {
        info.net_id_variant = ids.to_ours(info.net_id_variant);
    };
    for action in &mut request.actions {
        match action {
            Action::TakeActionData(a) => {
                fix(&mut a.source);
                fix(&mut a.destination);
            }
            Action::PlaceActionData(a) => {
                fix(&mut a.source);
                fix(&mut a.destination);
            }
            Action::SwapActionData(a) => {
                fix(&mut a.source);
                fix(&mut a.destination);
            }
            Action::DropActionData(a) => fix(&mut a.source),
            Action::DestroyActionData(a) => fix(&mut a.source),
            Action::ConsumeActionData(a) => fix(&mut a.source),
            Action::CraftRepairAndDisenchantActionData(a) => {
                a.recipe_net_id = ids.to_ours(a.recipe_net_id);
            }
            _ => {}
        }
    }
}

/// Compare our response with BDS's: status, container order, slot order,
/// amounts, names, durability, and stack-ID reuse versus fresh IDs.
pub fn assert_response(
    bds: &ItemStackResponseInfo,
    ours: ItemStackResponseInfo,
    ids: &mut IdMap,
    context: &str,
) {
    assert_eq!(
        ours.client_request_id.id, bds.client_request_id.id,
        "{context}"
    );
    assert_eq!(ours.result, bds.result, "{context}");
    let (theirs, ours) = (
        bds.containers.clone().unwrap_or_default(),
        ours.containers.unwrap_or_default(),
    );
    assert_eq!(ours.len(), theirs.len(), "{context}: containers");
    for (o, t) in ours.iter().zip(&theirs) {
        assert_eq!(o.full_container_name, t.full_container_name, "{context}");
        assert_eq!(o.slots.len(), t.slots.len(), "{context}: slots");
        for (o, t) in o.slots.iter().zip(&t.slots) {
            assert_eq!(
                (
                    o.requested_slot,
                    o.slot,
                    o.amount,
                    o.durability_correction,
                    &o.custom_name.unredacted
                ),
                (
                    t.requested_slot,
                    t.slot,
                    t.amount,
                    t.durability_correction,
                    &t.custom_name.unredacted
                ),
                "{context}"
            );
            match (&o.item_stack_net_id, &t.item_stack_net_id) {
                (Some(o), Some(t)) => ids.pair(t.id, o.id, context),
                (None, None) => {}
                other => panic!("{context}: stack IDs {other:?}"),
            }
        }
    }
}
