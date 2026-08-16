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
use crate::registry::ItemRegistry;
use jolyne::valentine::{
    ActorUniqueId, BlockPos, ContainerClosePacket, ContainerOpenPacket, EnumsContainerEnumName,
    EnumsInteractPacketPayloadAction, EnumsInventorySourceType, EnumsItemStackNetResult,
    EnumsItemUseInventoryTransactionActionType, InteractPacket,
    InventoryTransactionPacketTransaction, ItemStackRequestPacketDataRequestDataActionsItem,
    McpePacket,
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
    use jolyne::valentine::{
        BedrockSafetyRedactableString, FullContainerName, ItemStackResponseContainerInfo,
        ItemStackResponseInfo, ItemStackResponsePacket, ItemStackResponseSlotInfo,
        TypedClientNetIdstructItemStackRequestIdTagint32T0,
        TypedServerNetIdstructItemStackNetIdTagint32T0,
    };

    for request in &pk.requests {
        let request_id = request.client_request_id.id;
        info!(
            request_id,
            actions = request.actions.len(),
            "Processing ItemStackRequest"
        );

        let mut pending_item: Option<ItemStack> = None;
        let mut response_containers: Vec<ItemStackResponseContainerInfo> = vec![];

        for action in &request.actions {
            match action {
                ItemStackRequestPacketDataRequestDataActionsItem::CraftCreativeActionData(
                    craft,
                ) => {
                    let Some(index) = craft
                        .creative_item_net_id
                        .checked_sub(1)
                        .and_then(|value| usize::try_from(value).ok())
                    else {
                        debug!(
                            item_id = craft.creative_item_net_id,
                            "Creative item request has no valid index"
                        );
                        continue;
                    };
                    info!(
                        "CraftCreative: client requested item_id={}, index={}, total_items={}",
                        craft.creative_item_net_id,
                        index,
                        world_template.0.creative_content.entries.len()
                    );

                    if let Some(entry) = world_template.0.creative_content.entries.get(index) {
                        let item = item_stack_from_network_id(&items.0, entry.item_instance.id, 64);
                        info!(
                            item_id = craft.creative_item_net_id,
                            network_id = entry.item_instance.id,
                            string_id = %item.item_id,
                            "Creative craft request"
                        );
                        pending_item = Some(item);
                    } else {
                        debug!(
                            item_id = craft.creative_item_net_id,
                            "Creative item not found"
                        );
                    }
                }
                ItemStackRequestPacketDataRequestDataActionsItem::PlaceActionData(place) => {
                    if let Some(item) = pending_item.take() {
                        let dest_slot = place.destination.slot as usize;
                        let dest_container = place.destination.fullcontainername.container_name;
                        let dynamic_id = place.destination.fullcontainername.dynamic_id;

                        let stack_id = if let Ok((_, _, _, mut state, _)) = players.get_mut(entity)
                        {
                            state.next_id()
                        } else {
                            1
                        };

                        if dest_container
                            == EnumsContainerEnumName::CombinedHotbarAndInventoryContainer
                            && let Ok((mut inv, _, _, _, _)) = players.get_mut(entity)
                        {
                            let _ = inv.0.set_item(dest_slot, item.clone());
                            info!(slot = dest_slot, item_id = %item.item_id, stack_id, "Placed item in inventory");
                        }

                        response_containers.push(ItemStackResponseContainerInfo {
                            full_container_name: FullContainerName {
                                container_name: dest_container,
                                dynamic_id,
                            },
                            slots: vec![ItemStackResponseSlotInfo {
                                requested_slot: dest_slot as u8,
                                slot: dest_slot as u8,
                                amount: item.count,
                                item_stack_net_id: Some(Some(
                                    TypedServerNetIdstructItemStackNetIdTagint32T0 { id: stack_id },
                                )),
                                custom_name: BedrockSafetyRedactableString {
                                    unredacted: String::new(),
                                    redacted: String::new(),
                                },
                                durability_correction: 0,
                            }],
                        });
                    }
                }
                ItemStackRequestPacketDataRequestDataActionsItem::TakeActionData(take) => {
                    debug!(
                        source_slot = take.source.slot,
                        dest_slot = take.destination.slot,
                        count = take.amount,
                        "Take action (not fully implemented)"
                    );
                }
                ItemStackRequestPacketDataRequestDataActionsItem::DestroyActionData(_) => {
                    debug!("Destroy action - item deleted");
                }
                _ => {
                    trace!(action = ?action, "Unhandled ItemStackRequest action type");
                }
            }
        }

        // Send response
        if let Ok((_, _, _, _, session)) = players.get(entity) {
            let response = ItemStackResponsePacket {
                responses: vec![ItemStackResponseInfo {
                    result: EnumsItemStackNetResult::Success,
                    client_request_id: TypedClientNetIdstructItemStackRequestIdTagint32T0 {
                        id: request_id,
                    },
                    containers: Some(Some(response_containers)),
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
    debug!(entity = ?entity, container_id = pk.container_id, "ContainerClose");

    if pk.container_id == 0 {
        if let Ok((_, _, _, _, session)) = players.get(entity) {
            let _ = session.send(McpePacket::from(ContainerClosePacket {
                container_id: 0,
                container_type: u8::MAX,
                server_initiated_close: false,
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
    debug!(entity = ?entity, action = ?pk.action, "Interact packet");

    match pk.action {
        EnumsInteractPacketPayloadAction::OpenInventory => {
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
                    container_id: 0,
                    container_type: u8::MAX,
                    position: BlockPos { x: 0, y: 0, z: 0 },
                    target_actor_id: ActorUniqueId {
                        actor_unique_id: -1,
                    },
                }));
            }

            // Mark opened
            if let Ok((_, _, mut opened, _, _)) = players.get_mut(entity) {
                opened.0 = true;
            }
        }
        EnumsInteractPacketPayloadAction::InteractUpdate => {}
        _ => {
            debug!(entity = ?entity, action = ?pk.action, "Unhandled Interact action");
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
    let Some(transaction) = &pk.transaction else {
        debug!("InventoryTransaction without transaction data");
        return;
    };

    info!(transaction = ?transaction, "InventoryTransaction");

    match transaction {
        InventoryTransactionPacketTransaction::NormalTransactionData(normal) => {
            if let Some(actions) = normal.actions.actions.as_deref() {
                handle_normal_transaction(entity, actions, items, players);
            }
        }
        InventoryTransactionPacketTransaction::InventoryMismatchData(_) => {
            debug!("Inventory mismatch - should resend inventory");
        }
        InventoryTransactionPacketTransaction::ItemUseInventoryTransaction(use_item) => {
            match use_item.action_type {
                EnumsItemUseInventoryTransactionActionType::Use => {
                    events.0.push(ServerEvent::PlayerItemUse { entity });
                }
                _ => {
                    debug!(action = ?use_item.action_type, "Unhandled ItemUse action in inventory system");
                }
            }
        }
        InventoryTransactionPacketTransaction::ItemUseOnActorInventoryTransaction(_) => {
            debug!("ItemUseOnEntity transaction");
        }
        InventoryTransactionPacketTransaction::ItemReleaseInventoryTransaction(_) => {
            debug!("ItemRelease transaction");
        }
    }
}

fn handle_normal_transaction(
    entity: Entity,
    actions: &[jolyne::valentine::InventoryAction],
    items: &Res<super::types::ItemRegistryResource>,
    players: &mut Query<(
        &mut MainInventory,
        &mut HeldSlot,
        &mut InventoryOpened,
        &mut ItemStackRequestState,
        &PlayerSession,
    )>,
) {
    for action in actions {
        match action.source.source_type {
            EnumsInventorySourceType::CreativeInventory => {
                if action.to_item.id != 0 {
                    let count = action.to_item.stacksize;
                    info!(
                        network_id = action.to_item.id,
                        count, "Creative inventory pick"
                    );
                }
            }
            EnumsInventorySourceType::WorldInteraction => {}
            EnumsInventorySourceType::ContainerInventory => {
                let slot = action.slot as usize;

                if action.to_item.id != 0 {
                    let count = action.to_item.stacksize as u8;
                    let item =
                        item_stack_from_network_id(&items.0, i32::from(action.to_item.id), count);

                    if let Ok((mut inv, _, _, _, _)) = players.get_mut(entity) {
                        let _ = inv.0.set_item(slot, item);
                        info!(
                            slot,
                            network_id = action.to_item.id,
                            count,
                            window = ?action.source.container_id,
                            "Placed item in inventory from creative"
                        );
                    }
                }
            }
            other => {
                debug!(source_type = ?other, slot = action.slot, "Unhandled action source type");
            }
        }
    }
}

fn item_id_from_network_id(items: &ItemRegistry, network_id: i32) -> String {
    items
        .get_by_network_id(network_id)
        .map(|entry| entry.string_id.clone())
        .unwrap_or_else(|| {
            warn!("Item network_id {} not found in registry", network_id);
            format!("minecraft:network_{network_id}")
        })
}

fn item_stack_from_network_id(items: &ItemRegistry, network_id: i32, count: u8) -> ItemStack {
    if let Some(entry) = items.get_by_network_id(network_id) {
        return ItemStack::new(entry.string_id.clone(), count)
            .with_max_stack_size(entry.stack_size);
    }

    let item_id = item_id_from_network_id(items, network_id);
    ItemStack::new(item_id, count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::item::ItemEntry;

    #[test]
    fn item_id_lookup_uses_protocol_network_id_not_internal_id() {
        let mut items = ItemRegistry::new();
        items
            .register(ItemEntry {
                id: 99,
                network_id: -12,
                component_based: false,
                version: 0,
                string_id: "minecraft:negative_network_item".to_string(),
                name: "negative_network_item".to_string(),
                stack_size: 64,
            })
            .expect("register item");

        assert_eq!(
            item_id_from_network_id(&items, -12),
            "minecraft:negative_network_item"
        );
    }

    #[test]
    fn item_id_lookup_falls_back_with_signed_network_id() {
        let items = ItemRegistry::new();

        assert_eq!(
            item_id_from_network_id(&items, -12),
            "minecraft:network_-12"
        );
    }

    #[test]
    fn item_stack_lookup_caches_registry_stack_size() {
        let mut items = ItemRegistry::new();
        items
            .register(ItemEntry {
                id: 5,
                network_id: 77,
                component_based: false,
                version: 0,
                string_id: "minecraft:honey_bottle".to_string(),
                name: "honey bottle".to_string(),
                stack_size: 16,
            })
            .expect("register item");

        let stack = item_stack_from_network_id(&items, 77, 64);
        assert_eq!(stack.item_id, "minecraft:honey_bottle");
        assert_eq!(stack.count, 16);
        assert_eq!(stack.max_stack_size(), 16);
    }
}
