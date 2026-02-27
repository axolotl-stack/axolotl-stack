//! Inventory domain system.
//!
//! Processes ItemStackRequest, ContainerClose, MobEquipment, Interact,
//! and InventoryTransaction packets.

use bevy_ecs::prelude::*;
use tracing::{debug, info, trace, warn};

use super::packet_queues::{InventoryAction, InventoryEvents, InventoryPacketQueue};
use crate::ecs::events::ServerEvent;
use crate::entity::components::{
    HeldSlot, InventoryOpened, ItemStackRequestState, MainInventory, PlayerSession,
};
use crate::item::ItemStack;
use jolyne::valentine::types::{BlockCoordinates, ContainerSlotType, WindowId, WindowType};
use jolyne::valentine::{
    ContainerClosePacket, ContainerOpenPacket, InteractPacket, InteractPacketActionId, McpePacket,
};

/// ECS system: drain inventory packet queue and apply state changes.
///
/// Runs in `PacketApplySet`. Can execute in parallel with movement and chunks
/// because it writes disjoint components (MainInventory, HeldSlot, InventoryOpened,
/// ItemStackRequestState).
pub fn apply_inventory(
    mut queue: ResMut<InventoryPacketQueue>,
    mut events: ResMut<InventoryEvents>,
    items: Res<super::types::ItemRegistryResource>,
    world_template: Res<super::types::ServerWorldTemplate>,
    mut players: Query<(
        &mut MainInventory,
        &mut HeldSlot,
        &mut InventoryOpened,
        &mut ItemStackRequestState,
        &PlayerSession,
    )>,
) {
    let _span = tracing::info_span!("apply_inventory", count = queue.0.len()).entered();
    for (entity, action) in queue.0.drain(..) {
        match action {
            InventoryAction::ItemStackRequest(pk) => {
                handle_item_stack_request(entity, &pk, &items, &world_template, &mut players);
            }
            InventoryAction::ContainerClose(pk) => {
                handle_container_close(entity, &pk, &mut players);
            }
            InventoryAction::MobEquipment(pk) => {
                handle_mob_equipment(entity, &pk, &mut players, &mut events);
            }
            InventoryAction::Interact(pk) => {
                handle_interact(entity, &pk, &mut players);
            }
            InventoryAction::Transaction(pk) => {
                handle_inventory_transaction(entity, &pk, &items, &mut players, &mut events);
            }
        }
    }
}

fn handle_item_stack_request(
    entity: Entity,
    pk: &jolyne::valentine::ItemStackRequestPacket,
    items: &Res<super::types::ItemRegistryResource>,
    world_template: &Res<super::types::ServerWorldTemplate>,
    players: &mut Query<(
        &mut MainInventory,
        &mut HeldSlot,
        &mut InventoryOpened,
        &mut ItemStackRequestState,
        &PlayerSession,
    )>,
) {
    use jolyne::valentine::ItemStackResponsePacket;
    use jolyne::valentine::types::{
        FullContainerName, ItemStackRequestActionsItemContent, ItemStackResponsesItem,
        ItemStackResponsesItemContent, ItemStackResponsesItemContentContainersItem,
        ItemStackResponsesItemContentContainersItemSlotsItem, ItemStackResponsesItemStatus,
    };

    for request in &pk.requests {
        let request_id = request.request_id;
        info!(
            request_id,
            actions = request.actions.len(),
            "Processing ItemStackRequest"
        );

        let mut pending_item: Option<ItemStack> = None;
        let mut response_containers: Vec<ItemStackResponsesItemContentContainersItem> = vec![];

        for action in &request.actions {
            match &action.content {
                Some(ItemStackRequestActionsItemContent::CraftCreative(craft)) => {
                    let index = (craft.item_id - 1) as usize;
                    info!(
                        "CraftCreative: client requested item_id={}, index={}, total_items={}",
                        craft.item_id,
                        index,
                        world_template.0.creative_content.items.len()
                    );

                    if let Some(entry) = world_template.0.creative_content.items.get(index) {
                        let item_id = items
                            .0
                            .get(entry.item.network_id as u32)
                            .map(|e| e.string_id.clone())
                            .unwrap_or_else(|| {
                                warn!(
                                    "Creative item network_id {} not found in registry",
                                    entry.item.network_id
                                );
                                format!("minecraft:network_{}", entry.item.network_id)
                            });

                        let item = ItemStack::new(item_id.clone(), 64);
                        info!(
                            item_id = craft.item_id,
                            network_id = entry.item.network_id,
                            string_id = %item_id,
                            "Creative craft request"
                        );
                        pending_item = Some(item);
                    } else {
                        debug!(item_id = craft.item_id, "Creative item not found");
                    }
                }
                Some(ItemStackRequestActionsItemContent::Place(place)) => {
                    if let Some(item) = pending_item.take() {
                        let dest_slot = place.destination.slot as usize;
                        let dest_container = place.destination.slot_type.container_id;

                        let stack_id = if let Ok((_, _, _, mut state, _)) = players.get_mut(entity)
                        {
                            state.next_id()
                        } else {
                            1
                        };

                        if dest_container == ContainerSlotType::HotbarAndInventory
                            && let Ok((mut inv, _, _, _, _)) = players.get_mut(entity)
                        {
                            let _ = inv.0.set_item(dest_slot, item.clone());
                            info!(slot = dest_slot, item_id = %item.item_id, stack_id, "Placed item in inventory");
                        }

                        response_containers.push(ItemStackResponsesItemContentContainersItem {
                            slot_type: FullContainerName {
                                container_id: dest_container,
                                dynamic_container_id: place
                                    .destination
                                    .slot_type
                                    .dynamic_container_id,
                            },
                            slots: vec![ItemStackResponsesItemContentContainersItemSlotsItem {
                                slot: dest_slot as u8,
                                hotbar_slot: dest_slot as u8,
                                count: item.count,
                                item_stack_id: stack_id,
                                custom_name: String::new(),
                                filtered_custom_name: String::new(),
                                durability_correction: 0,
                            }],
                        });
                    }
                }
                Some(ItemStackRequestActionsItemContent::Take(take)) => {
                    debug!(
                        source_slot = take.source.slot,
                        dest_slot = take.destination.slot,
                        count = take.count,
                        "Take action (not fully implemented)"
                    );
                }
                Some(ItemStackRequestActionsItemContent::Destroy(_)) => {
                    debug!("Destroy action - item deleted");
                }
                _ => {
                    trace!(type_id = ?action.type_id, "Unhandled ItemStackRequest action type");
                }
            }
        }

        // Send response
        if let Ok((_, _, _, _, session)) = players.get(entity) {
            let response = ItemStackResponsePacket {
                responses: vec![ItemStackResponsesItem {
                    status: ItemStackResponsesItemStatus::Ok,
                    request_id,
                    content: Some(ItemStackResponsesItemContent {
                        containers: response_containers,
                    }),
                }],
            };
            let _ = session.send(McpePacket::from(response));
            debug!(request_id, "Sent ItemStackResponse");
        }
    }
}

fn handle_container_close(
    entity: Entity,
    pk: &ContainerClosePacket,
    players: &mut Query<(
        &mut MainInventory,
        &mut HeldSlot,
        &mut InventoryOpened,
        &mut ItemStackRequestState,
        &PlayerSession,
    )>,
) {
    debug!(entity = ?entity, window_id = ?pk.window_id, "ContainerClose");

    if pk.window_id == WindowId::Inventory {
        if let Ok((_, _, _, _, session)) = players.get(entity) {
            let _ = session.send(McpePacket::from(ContainerClosePacket {
                window_id: WindowId::Inventory,
                window_type: WindowType::Inventory,
                server: false,
            }));
        }
        if let Ok((_, _, mut opened, _, _)) = players.get_mut(entity) {
            opened.0 = false;
        }
    }
}

fn handle_mob_equipment(
    entity: Entity,
    pk: &jolyne::valentine::MobEquipmentPacket,
    players: &mut Query<(
        &mut MainInventory,
        &mut HeldSlot,
        &mut InventoryOpened,
        &mut ItemStackRequestState,
        &PlayerSession,
    )>,
    events: &mut ResMut<InventoryEvents>,
) {
    if let Ok((_, mut held_slot, _, _, _)) = players.get_mut(entity) {
        let old_slot = held_slot.0;
        if old_slot != pk.selected_slot {
            held_slot.set(pk.selected_slot);
            trace!(entity = ?entity, slot = pk.selected_slot, "Player changed held slot");

            events.0.push(ServerEvent::PlayerHeldSlotChange {
                entity,
                old_slot,
                new_slot: pk.selected_slot,
            });
        }
    }
}

fn handle_interact(
    entity: Entity,
    pk: &InteractPacket,
    players: &mut Query<(
        &mut MainInventory,
        &mut HeldSlot,
        &mut InventoryOpened,
        &mut ItemStackRequestState,
        &PlayerSession,
    )>,
) {
    debug!(entity = ?entity, action = ?pk.action_id, "Interact packet");

    match pk.action_id {
        InteractPacketActionId::OpenInventory => {
            // Check if already open
            if let Ok((_, _, opened, _, _)) = players.get(entity)
                && opened.0
            {
                debug!(entity = ?entity, "Inventory already open, skipping");
                return;
            }

            // Send ContainerOpen
            if let Ok((_, _, _, _, session)) = players.get(entity) {
                let _ = session.send(McpePacket::from(ContainerOpenPacket {
                    window_id: WindowId::Inventory,
                    window_type: WindowType::Inventory,
                    coordinates: BlockCoordinates { x: 0, y: 0, z: 0 },
                    runtime_entity_id: -1,
                }));
            }

            // Mark opened
            if let Ok((_, _, mut opened, _, _)) = players.get_mut(entity) {
                opened.0 = true;
            }
        }
        InteractPacketActionId::MouseOverEntity => {}
        _ => {
            debug!(entity = ?entity, action = ?pk.action_id, "Unhandled Interact action");
        }
    }
}

fn handle_inventory_transaction(
    entity: Entity,
    pk: &jolyne::valentine::InventoryTransactionPacket,
    items: &Res<super::types::ItemRegistryResource>,
    players: &mut Query<(
        &mut MainInventory,
        &mut HeldSlot,
        &mut InventoryOpened,
        &mut ItemStackRequestState,
        &PlayerSession,
    )>,
    events: &mut ResMut<InventoryEvents>,
) {
    use jolyne::valentine::types::{
        TransactionTransactionData, TransactionTransactionType, TransactionUseItemActionType,
    };

    let transaction = &pk.transaction;
    info!(transaction_type = ?transaction.transaction_type, "InventoryTransaction");

    match transaction.transaction_type {
        TransactionTransactionType::Normal => {
            handle_normal_transaction(entity, &transaction.actions, items, players);
        }
        TransactionTransactionType::InventoryMismatch => {
            debug!("Inventory mismatch - should resend inventory");
        }
        TransactionTransactionType::ItemUse => {
            if let Some(TransactionTransactionData::ItemUse(use_item)) =
                &transaction.transaction_data
            {
                match use_item.action_type {
                    TransactionUseItemActionType::ClickAir => {
                        events.0.push(ServerEvent::PlayerItemUse { entity });
                    }
                    _ => {
                        debug!(action = ?use_item.action_type, "Unhandled ItemUse action in inventory system");
                    }
                }
            }
        }
        TransactionTransactionType::ItemUseOnEntity => {
            debug!("ItemUseOnEntity transaction");
        }
        TransactionTransactionType::ItemRelease => {
            debug!("ItemRelease transaction");
        }
        other => {
            debug!(transaction_type = ?other, "Unhandled transaction type");
        }
    }
}

fn handle_normal_transaction(
    entity: Entity,
    actions: &[jolyne::valentine::types::TransactionActionsItem],
    items: &Res<super::types::ItemRegistryResource>,
    players: &mut Query<(
        &mut MainInventory,
        &mut HeldSlot,
        &mut InventoryOpened,
        &mut ItemStackRequestState,
        &PlayerSession,
    )>,
) {
    use jolyne::valentine::types::TransactionActionsItemSourceType;

    for action in actions {
        match action.source_type {
            TransactionActionsItemSourceType::Creative => {
                if action.new_item.network_id != 0 {
                    let count = action
                        .new_item
                        .content
                        .as_ref()
                        .map(|c| c.count)
                        .unwrap_or(1);
                    info!(
                        network_id = action.new_item.network_id,
                        count, "Creative inventory pick"
                    );
                }
            }
            TransactionActionsItemSourceType::WorldInteraction => {}
            TransactionActionsItemSourceType::Container => {
                if let Some(content) = &action.content {
                    use jolyne::valentine::types::TransactionActionsItemContent;
                    if let TransactionActionsItemContent::Container(container) = content {
                        let slot = action.slot as usize;

                        if action.new_item.network_id != 0 {
                            let count = action
                                .new_item
                                .content
                                .as_ref()
                                .map(|c| c.count as u8)
                                .unwrap_or(1);

                            let item_id = items
                                .0
                                .get(action.new_item.network_id as u32)
                                .map(|e| e.string_id.clone())
                                .unwrap_or_else(|| {
                                    format!("minecraft:network_{}", action.new_item.network_id)
                                });

                            let item = ItemStack::new(item_id.clone(), count);

                            if let Ok((mut inv, _, _, _, _)) = players.get_mut(entity) {
                                let _ = inv.0.set_item(slot, item);
                                info!(
                                    slot,
                                    network_id = action.new_item.network_id,
                                    count,
                                    window = ?container.inventory_id,
                                    "Placed item in inventory from creative"
                                );
                            }
                        }
                    }
                }
            }
            other => {
                debug!(source_type = ?other, slot = action.slot, "Unhandled action source type");
            }
        }
    }
}
