//! Inventory domain system.
//!
//! Processes ItemStackRequest, ContainerClose, MobEquipment, Interact,
//! and InventoryTransaction packets.

use bevy_ecs::prelude::*;
use tracing::{debug, info, trace};

use super::packet_queues::{InventoryAction, InventoryEvents, InventoryPacketQueue};
use crate::ecs::events::ServerEvent;
use crate::entity::components::{
    GameMode, HeldSlot, Inventory, InventoryOpened, ItemStackRequestState, MainInventory,
    PlayerSession,
};
use crate::item::{ItemStack, user_data};
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
    modes: Query<&GameMode>,
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
                let mode = modes.get(entity).copied().unwrap_or_default();
                handle_item_stack_request(entity, &pk, &items, &world_template, mode, &mut players);
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
                let mode = modes.get(entity).copied().unwrap_or_default();
                handle_inventory_transaction(entity, &pk, &items, mode, &mut players, &mut events);
            }
        }
    }
}

fn handle_item_stack_request(
    entity: Entity,
    pk: &jolyne::valentine::ItemStackRequestPacket,
    items: &Res<super::types::ItemRegistryResource>,
    world_template: &Res<super::types::ServerWorldTemplate>,
    mode: GameMode,
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
        let Ok((mut inv, _, _, mut state, session)) = players.get_mut(entity) else {
            continue;
        };
        // Only the fully implemented creative-output -> empty main slot operation
        // is accepted. Validate the complete request before changing any state.
        let placement = validate_creative_request(&inv.0, &request.actions, &items.0, mode, |id| {
            Some(
                &world_template
                    .0
                    .creative_content
                    .entries
                    .iter()
                    .find(|entry| entry.creative_net_id.id == id)?
                    .item_instance,
            )
        });
        let mut containers = None;
        let result = if let Some((slot, item)) = placement {
            let amount = item.count;
            inv.0
                .set_item(slot, item)
                .expect("validated inventory slot");
            let stack_id = state.next_id();
            containers = Some(vec![ItemStackResponseContainerInfo {
                full_container_name: FullContainerName {
                    container_name: EnumsContainerEnumName::Combinedhotbarandinventorycontainer,
                    dynamic_id: None,
                },
                slots: vec![ItemStackResponseSlotInfo {
                    requested_slot: slot as u8,
                    slot: slot as u8,
                    amount,
                    item_stack_net_id: Some(TypedServerNetIdstructItemStackNetIdTagint32T0 {
                        id: stack_id,
                    }),
                    custom_name: BedrockSafetyRedactableString {
                        unredacted: "".into(),
                        redacted: None,
                    },
                    durability_correction: 0,
                }],
            }]);
            EnumsItemStackNetResult::Success
        } else {
            send_inventory(&inv.0, session, &items.0);
            EnumsItemStackNetResult::Error
        };
        let _ = session.send(McpePacket::from(ItemStackResponsePacket {
            responses: vec![ItemStackResponseInfo {
                result,
                client_request_id: TypedClientNetIdstructItemStackRequestIdTagint32T0 {
                    id: request.client_request_id.id,
                },
                containers,
            }],
        }));
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
        EnumsInteractPacketPayloadAction::Openinventory => {
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
        EnumsInteractPacketPayloadAction::Interactupdate => {}
        _ => {
            debug!(entity = ?entity, action = ?pk.action, "Unhandled Interact action");
        }
    }
}

fn handle_inventory_transaction(
    entity: Entity,
    pk: &jolyne::valentine::InventoryTransactionPacket,
    items: &Res<super::types::ItemRegistryResource>,
    mode: GameMode,
    players: &mut Query<(
        &mut MainInventory,
        &mut HeldSlot,
        &mut InventoryOpened,
        &mut ItemStackRequestState,
        &PlayerSession,
    )>,
    events: &mut ResMut<InventoryEvents>,
) {
    let transaction = &pk.transaction;

    info!(transaction = ?transaction, "InventoryTransaction");

    match transaction {
        InventoryTransactionPacketTransaction::NormalTransactionData(normal) => {
            handle_normal_transaction(entity, &normal.actions.actions, items, mode, players);
        }
        InventoryTransactionPacketTransaction::InventoryMismatchData(_) => {
            if let Ok((inv, _, _, _, session)) = players.get(entity) {
                send_inventory(&inv.0, session, &items.0);
            }
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
    mode: GameMode,
    players: &mut Query<(
        &mut MainInventory,
        &mut HeldSlot,
        &mut InventoryOpened,
        &mut ItemStackRequestState,
        &PlayerSession,
    )>,
) {
    if let Ok((mut inv, _, _, _, session)) = players.get_mut(entity)
        && !apply_normal_actions(&mut inv.0, actions, &items.0, mode)
    {
        send_inventory(&inv.0, session, &items.0);
    }
}

type WireItem = jolyne::valentine::CerealizerNetworkItemStackDescriptorSerializedData;
type CreativeItem = jolyne::valentine::CerealizerNetworkItemInstanceDescriptorSerializedData;

fn wire_item(stack: &ItemStack, items: &ItemRegistry) -> Option<WireItem> {
    if stack.is_empty() {
        return Some(WireItem::default());
    }
    Some(WireItem {
        id: i16::try_from(items.get_by_name(&stack.item_id)?.network_id).ok()?,
        stacksize: u16::from(stack.count),
        auxvalue: u32::try_from(stack.damage).ok()?,
        net_id_variant: None,
        block_runtime_id: u32::try_from(stack.runtime_id).ok()?,
        user_data_buffer: user_data::network_user_data(&stack.item_id, stack.nbt.as_deref()),
    })
}

fn validated_item(wire: &WireItem, items: &ItemRegistry) -> Option<ItemStack> {
    if wire.id == 0 {
        return (wire.stacksize == 0
            && wire.auxvalue == 0
            && wire.block_runtime_id == 0
            && wire.user_data_buffer.is_empty())
        .then(ItemStack::empty);
    }
    let entry = items.get_by_network_id(i32::from(wire.id))?;
    if wire.stacksize == 0 || wire.stacksize > u16::from(entry.stack_size) {
        return None;
    }
    Some(ItemStack {
        item_id: entry.string_id.clone(),
        runtime_id: i32::try_from(wire.block_runtime_id).ok()?,
        count: u8::try_from(wire.stacksize).ok()?,
        damage: i16::try_from(wire.auxvalue).ok()?,
        nbt: user_data::stored_user_data(&entry.string_id, &wire.user_data_buffer),
        max_stack_size_override: Some(entry.stack_size),
    })
}

/// Validate against the original inventory, then commit all slots together.
/// Duplicate slots and non-main containers are unsupported, not implicit grants.
fn apply_normal_actions(
    inv: &mut Inventory,
    actions: &[jolyne::valentine::InventoryAction],
    items: &ItemRegistry,
    mode: GameMode,
) -> bool {
    let mut changes: Vec<(usize, ItemStack)> = Vec::with_capacity(actions.len().min(inv.size()));
    if actions.len() > inv.size() {
        return false;
    }
    for action in actions {
        if action.source.source_type != EnumsInventorySourceType::Containerinventory
            || action.source.container_id != Some(0)
            || action.source.bit_flags.is_some()
        {
            return false;
        }
        let Ok(slot) = usize::try_from(action.slot) else {
            return false;
        };
        let Some(actual) = inv.item(slot) else {
            return false;
        };
        if changes.iter().any(|(previous, _)| *previous == slot) {
            return false;
        }
        let Some(expected) = wire_item(actual, items) else {
            return false;
        };
        // Stack prediction IDs are transport bookkeeping, not item identity;
        // user data compares decoded (an omitted buffer equals the default).
        let expected = WireItem {
            net_id_variant: action.from_item.net_id_variant,
            user_data_buffer: action.from_item.user_data_buffer.clone(),
            ..expected
        };
        if expected != action.from_item
            || user_data::stored_user_data(&actual.item_id, &action.from_item.user_data_buffer)
                != actual.nbt
            || validated_item(&action.from_item, items).is_none()
        {
            return false;
        }
        let Some(item) = validated_item(&action.to_item, items) else {
            return false;
        };
        changes.push((slot, item));
    }
    if mode != GameMode::Creative {
        // Compare every affected item identity (including damage and NBT).
        // The bounded main inventory keeps this allocation-free tally small.
        for identity in changes
            .iter()
            .flat_map(|(slot, item)| [inv.item(*slot).unwrap(), item])
        {
            if identity.is_empty() {
                continue;
            }
            let before: u32 = changes
                .iter()
                .map(|(slot, _)| inv.item(*slot).unwrap())
                .filter(|item| item.comparable(identity) && item.runtime_id == identity.runtime_id)
                .map(|item| u32::from(item.count))
                .sum();
            let after: u32 = changes
                .iter()
                .map(|(_, item)| item)
                .filter(|item| item.comparable(identity) && item.runtime_id == identity.runtime_id)
                .map(|item| u32::from(item.count))
                .sum();
            if before != after {
                return false;
            }
        }
    }
    for (slot, item) in changes {
        inv.set_item(slot, item).expect("validated inventory slot");
    }
    true
}

fn creative_stack(
    items: &ItemRegistry,
    network_id: i32,
    count: u8,
    mode: GameMode,
) -> Option<ItemStack> {
    if mode != GameMode::Creative {
        return None;
    }
    let entry = items.get_by_network_id(network_id)?;
    if count == 0 || count > entry.stack_size {
        return None;
    }
    Some(ItemStack::new(entry.string_id.clone(), count).with_max_stack_size(entry.stack_size))
}

fn validate_creative_request<'a>(
    inv: &Inventory,
    actions: &[ItemStackRequestPacketDataRequestDataActionsItem],
    items: &ItemRegistry,
    mode: GameMode,
    creative_item: impl FnOnce(u32) -> Option<&'a CreativeItem>,
) -> Option<(usize, ItemStack)> {
    let [
        ItemStackRequestPacketDataRequestDataActionsItem::CraftCreativeActionData(craft),
        ItemStackRequestPacketDataRequestDataActionsItem::PlaceActionData(place),
    ] = actions
    else {
        return None;
    };
    if mode != GameMode::Creative
        || craft.numberofrequestedcrafts != 1
        || place.source.fullcontainername.container_name
            != EnumsContainerEnumName::Createdoutputcontainer
        || place.source.fullcontainername.dynamic_id.is_some()
        || place.destination.fullcontainername.container_name
            != EnumsContainerEnumName::Combinedhotbarandinventorycontainer
        || place.destination.fullcontainername.dynamic_id.is_some()
    {
        return None;
    }
    let slot = usize::from(place.destination.slot);
    if !inv.item(slot)?.is_empty() {
        return None;
    }
    let source = creative_item(craft.creative_item_net_id)?;
    let mut item = creative_stack(items, source.id, place.amount, mode)?;
    u32::try_from(source.block_runtime_id).ok()?;
    item.runtime_id = source.block_runtime_id;
    item.damage = i16::try_from(source.auxvalue).ok()?;
    item.nbt = user_data::stored_user_data(&item.item_id, &source.user_data_buffer);
    Some((slot, item))
}

fn send_inventory(inv: &Inventory, session: &PlayerSession, items: &ItemRegistry) {
    let Some(slots) = (0..inv.size())
        .map(|slot| wire_item(inv.item(slot).unwrap(), items))
        .collect::<Option<Vec<_>>>()
    else {
        tracing::warn!("Cannot resync inventory containing an unrepresentable server item");
        return;
    };
    let _ = session.send(McpePacket::from(
        jolyne::valentine::InventoryContentPacket {
            container_id: 0,
            slots,
            full_container_name: jolyne::valentine::FullContainerName {
                container_name: EnumsContainerEnumName::Combinedhotbarandinventorycontainer,
                dynamic_id: None,
            },
            storage_item: WireItem::default(),
        },
    ));
}

/// Correct client prediction after a rejected gameplay inventory mutation.
pub(super) fn resync_inventory(world: &World, entity: Entity) {
    if let (Some(inv), Some(session), Some(items)) = (
        world.get::<MainInventory>(entity),
        world.get::<PlayerSession>(entity),
        world.get_resource::<super::types::ItemRegistryResource>(),
    ) {
        send_inventory(&inv.0, session, &items.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::components::Inventory;
    use crate::registry::item::ItemEntry;

    fn registry() -> ItemRegistry {
        let mut items = ItemRegistry::new();
        items
            .register(ItemEntry {
                id: 5,
                network_id: 77,
                component_based: false,
                version: 0,
                string_id: "minecraft:honey_bottle".into(),
                name: "honey bottle".into(),
                stack_size: 16,
            })
            .unwrap();
        items
    }

    fn descriptor(
        count: u16,
    ) -> jolyne::valentine::CerealizerNetworkItemStackDescriptorSerializedData {
        jolyne::valentine::CerealizerNetworkItemStackDescriptorSerializedData {
            id: if count == 0 { 0 } else { 77 },
            stacksize: count,
            ..Default::default()
        }
    }

    fn action(slot: u32, from: u16, to: u16) -> jolyne::valentine::InventoryAction {
        jolyne::valentine::InventoryAction {
            source: jolyne::valentine::InventorySource {
                source_type: EnumsInventorySourceType::Containerinventory,
                container_id: Some(0),
                ..Default::default()
            },
            slot,
            from_item: descriptor(from),
            to_item: descriptor(to),
        }
    }

    #[test]
    fn forged_stacks_are_rejected_without_truncation() {
        let items = registry();
        for count in [17, 256, 272, u16::MAX] {
            let mut inv = Inventory::new(2);
            assert!(!apply_normal_actions(
                &mut inv,
                &[action(0, 0, count)],
                &items,
                GameMode::Creative
            ));
            assert!(inv.item(0).unwrap().is_empty());
        }
        let mut inv = Inventory::new(2);
        let mut forged = action(0, 0, 1);
        forged.to_item.id = -12;
        assert!(!apply_normal_actions(
            &mut inv,
            &[forged],
            &items,
            GameMode::Creative
        ));
    }

    #[test]
    fn stale_from_item_rejects_entire_transaction() {
        let items = registry();
        let mut inv = Inventory::new(2);
        inv.set_item(0, ItemStack::new("minecraft:honey_bottle", 8))
            .unwrap();
        let actions = [action(0, 8, 0), action(1, 1, 8)];
        assert!(!apply_normal_actions(
            &mut inv,
            &actions,
            &items,
            GameMode::Survival
        ));
        assert_eq!(inv.item(0).unwrap().count, 8);
        assert!(inv.item(1).unwrap().is_empty());
    }

    #[test]
    fn survival_moves_conserve_items_and_clear_source() {
        let items = registry();
        let mut inv = Inventory::new(2);
        inv.set_item(0, ItemStack::new("minecraft:honey_bottle", 8))
            .unwrap();
        assert!(!apply_normal_actions(
            &mut inv,
            &[action(1, 0, 8)],
            &items,
            GameMode::Survival
        ));
        assert!(apply_normal_actions(
            &mut inv,
            &[action(0, 8, 0), action(1, 0, 8)],
            &items,
            GameMode::Survival
        ));
        assert!(inv.item(0).unwrap().is_empty());
        assert_eq!(inv.item(1).unwrap().count, 8);
        assert_eq!(inv.item(1).unwrap().max_stack_size(), 16);
    }

    #[test]
    fn unsupported_sources_containers_and_repeated_slots_are_atomic() {
        let items = registry();
        for bad in [
            {
                let mut a = action(1, 0, 1);
                a.source.container_id = Some(119);
                a
            },
            {
                let mut a = action(1, 0, 1);
                a.source.source_type = EnumsInventorySourceType::Worldinteraction;
                a
            },
            action(99, 0, 1),
            action(0, 0, 1),
        ] {
            let mut inv = Inventory::new(2);
            assert!(!apply_normal_actions(
                &mut inv,
                &[action(0, 0, 1), bad],
                &items,
                GameMode::Creative
            ));
            assert!(inv.item(0).unwrap().is_empty());
        }
    }

    #[test]
    fn creative_creation_requires_server_creative_mode() {
        let items = registry();
        for mode in [GameMode::Survival, GameMode::Adventure, GameMode::Spectator] {
            let mut inv = Inventory::new(1);
            assert!(!apply_normal_actions(
                &mut inv,
                &[action(0, 0, 16)],
                &items,
                mode
            ));
            assert!(creative_stack(&items, 77, 16, mode).is_none());
        }
        let stack = creative_stack(&items, 77, 16, GameMode::Creative).unwrap();
        assert_eq!(stack.count, 16);
        assert!(creative_stack(&items, 77, 17, GameMode::Creative).is_none());
        assert!(creative_stack(&items, 900, 1, GameMode::Creative).is_none());
    }

    #[test]
    fn creative_request_rejects_unsupported_tail_and_survival() {
        use jolyne::valentine::{
            FullContainerName, ItemStackRequestCerealCraftCreativeActionData,
            ItemStackRequestCerealPlaceActionData, ItemStackRequestCerealSlotInfoData,
        };
        let items = registry();
        let inv = Inventory::new(2);
        let creative = CreativeItem {
            id: 77,
            ..Default::default()
        };
        let mut actions = vec![
            ItemStackRequestPacketDataRequestDataActionsItem::CraftCreativeActionData(
                ItemStackRequestCerealCraftCreativeActionData {
                    creative_item_net_id: 1,
                    numberofrequestedcrafts: 1,
                    ..Default::default()
                },
            ),
            ItemStackRequestPacketDataRequestDataActionsItem::PlaceActionData(
                ItemStackRequestCerealPlaceActionData {
                    amount: 16,
                    source: ItemStackRequestCerealSlotInfoData {
                        fullcontainername: FullContainerName {
                            container_name: EnumsContainerEnumName::Createdoutputcontainer,
                            dynamic_id: None,
                        },
                        ..Default::default()
                    },
                    destination: ItemStackRequestCerealSlotInfoData {
                        fullcontainername: FullContainerName {
                            container_name:
                                EnumsContainerEnumName::Combinedhotbarandinventorycontainer,
                            dynamic_id: None,
                        },
                        ..Default::default()
                    },
                    ..Default::default()
                },
            ),
        ];
        assert!(
            validate_creative_request(&inv, &actions, &items, GameMode::Survival, |_| Some(
                &creative
            ))
            .is_none()
        );
        let (slot, stack) =
            validate_creative_request(&inv, &actions, &items, GameMode::Creative, |_| {
                Some(&creative)
            })
            .unwrap();
        assert_eq!((slot, stack.count, stack.max_stack_size()), (0, 16, 16));
        actions.push(actions[1].clone());
        assert!(
            validate_creative_request(&inv, &actions, &items, GameMode::Creative, |_| Some(
                &creative
            ))
            .is_none()
        );
        assert!(inv.item(0).unwrap().is_empty());
    }

    #[test]
    fn survival_cannot_change_item_metadata_or_destroy_items() {
        let items = registry();
        let mut inv = Inventory::new(1);
        inv.set_item(0, ItemStack::new("minecraft:honey_bottle", 8))
            .unwrap();
        for forged in [
            {
                let mut a = action(0, 8, 8);
                a.to_item.auxvalue = 1;
                a
            },
            {
                let mut a = action(0, 8, 8);
                a.to_item.block_runtime_id = 1;
                a
            },
            {
                let mut a = action(0, 8, 8);
                a.to_item.user_data_buffer = vec![1].into();
                a
            },
            action(0, 8, 0),
        ] {
            assert!(!apply_normal_actions(
                &mut inv,
                &[forged],
                &items,
                GameMode::Survival
            ));
            assert_eq!(inv.item(0).unwrap().count, 8);
        }
    }

    fn run_creative_packet(
        mode: GameMode,
        amount: u8,
    ) -> (World, Entity, tokio::sync::mpsc::Receiver<McpePacket>) {
        use super::super::types::{ItemRegistryResource, ServerWorldTemplate};
        use jolyne::valentine::*;
        use std::sync::Arc;
        let mut world = World::new();
        world.insert_resource(ItemRegistryResource(Arc::new(registry())));
        let mut template = jolyne::WorldTemplate::default();
        let mut content = CreativeContentPacket::default();
        content.entries.push(CreativeItemEntryPayload {
            creative_net_id: TypedServerNetIdstructCreativeItemNetIdTag { id: 1 },
            item_instance: CerealizerNetworkItemInstanceDescriptorSerializedData {
                id: 77,
                stacksize: 1,
                auxvalue: 7,
                block_runtime_id: 33,
                user_data_buffer: vec![1, 2].into(),
            },
            ..Default::default()
        });
        template.creative_content = Arc::new(content);
        world.insert_resource(ServerWorldTemplate(Arc::new(template)));
        world.insert_resource(InventoryEvents::default());
        let (tx, rx) = tokio::sync::mpsc::channel(8);
        let entity = world
            .spawn((
                mode,
                MainInventory::default(),
                HeldSlot::default(),
                InventoryOpened::default(),
                ItemStackRequestState::default(),
                PlayerSession::new(1, "test".into(), None, None, tx),
            ))
            .id();
        let actions = vec![
            ItemStackRequestPacketDataRequestDataActionsItem::CraftCreativeActionData(
                ItemStackRequestCerealCraftCreativeActionData {
                    creative_item_net_id: 1,
                    numberofrequestedcrafts: 1,
                    ..Default::default()
                },
            ),
            ItemStackRequestPacketDataRequestDataActionsItem::PlaceActionData(
                ItemStackRequestCerealPlaceActionData {
                    amount,
                    source: ItemStackRequestCerealSlotInfoData {
                        fullcontainername: FullContainerName {
                            container_name: EnumsContainerEnumName::Createdoutputcontainer,
                            dynamic_id: None,
                        },
                        ..Default::default()
                    },
                    destination: ItemStackRequestCerealSlotInfoData {
                        fullcontainername: FullContainerName {
                            container_name:
                                EnumsContainerEnumName::Combinedhotbarandinventorycontainer,
                            dynamic_id: None,
                        },
                        ..Default::default()
                    },
                    ..Default::default()
                },
            ),
        ];
        world.insert_resource(InventoryPacketQueue(vec![(
            entity,
            super::InventoryAction::ItemStackRequest(ItemStackRequestPacket {
                requests: vec![ItemStackRequestPacketDataRequestData {
                    actions,
                    ..Default::default()
                }],
            }),
        )]));
        let mut schedule = Schedule::default();
        schedule.add_systems(apply_inventory);
        schedule.run(&mut world);
        (world, entity, rx)
    }

    #[test]
    fn creative_acquisition_preserves_advertised_metadata_for_later_moves() {
        let (mut world, entity, mut rx) = run_creative_packet(GameMode::Creative, 16);
        let jolyne::valentine::McpePacketData::ItemStackResponsePacket(response) =
            rx.try_recv().unwrap().data
        else {
            panic!("stack response")
        };
        assert_eq!(
            response.responses[0].result,
            EnumsItemStackNetResult::Success
        );
        let stack = world
            .get::<MainInventory>(entity)
            .unwrap()
            .0
            .item(0)
            .unwrap();
        assert_eq!(
            (stack.runtime_id, stack.damage, stack.nbt.as_deref()),
            (33, 7, Some([1, 2].as_slice()))
        );
        let mut from = descriptor(16);
        from.block_runtime_id = 33;
        from.auxvalue = 7;
        from.user_data_buffer = vec![1, 2].into();
        let mut remove = action(0, 16, 0);
        remove.from_item = from.clone();
        let mut insert = action(1, 0, 16);
        insert.to_item = from;
        let items = registry();
        let mut inventory = world.get_mut::<MainInventory>(entity).unwrap();
        assert!(apply_normal_actions(
            &mut inventory.0,
            &[remove, insert],
            &items,
            GameMode::Survival
        ));
        assert!(inventory.0.item(0).unwrap().is_empty());
        assert_eq!(inventory.0.item(1).unwrap().damage, 7);
    }

    #[test]
    fn forged_creative_count_sends_error_and_authoritative_empty_inventory() {
        for (mode, amount) in [(GameMode::Creative, 17), (GameMode::Survival, 16)] {
            let (world, entity, mut rx) = run_creative_packet(mode, amount);
            assert!(
                world
                    .get::<MainInventory>(entity)
                    .unwrap()
                    .0
                    .slots()
                    .iter()
                    .all(ItemStack::is_empty)
            );
            let jolyne::valentine::McpePacketData::InventoryContentPacket(content) =
                rx.try_recv().unwrap().data
            else {
                panic!("inventory correction")
            };
            assert!(
                content
                    .slots
                    .iter()
                    .all(|item| item.id == 0 && item.stacksize == 0)
            );
            let jolyne::valentine::McpePacketData::ItemStackResponsePacket(response) =
                rx.try_recv().unwrap().data
            else {
                panic!("stack response")
            };
            assert_eq!(response.responses[0].result, EnumsItemStackNetResult::Error);
            assert!(rx.try_recv().is_err());
        }
    }
}
