//! Packet handling dispatch and input processing.
//!
//! Routes incoming packets to appropriate handlers.

use bevy_ecs::entity::Entity;
use glam::DVec3;
use jolyne::valentine::ContainerSlotType;
use tracing::{debug, info, trace, warn};

use super::GameServer;
use super::types::SessionEntityMap;
use crate::entity::components::transform::{Position, Rotation};
use crate::entity::components::{
    HeldSlot, InventoryOpened, PlayerInput, PlayerSession, PlayerState,
};
use crate::network::SessionId;
use jolyne::valentine::types::{Action, BlockCoordinates, InputFlag, WindowId, WindowType};
use jolyne::valentine::{
    AnimatePacket, ContainerClosePacket, ContainerOpenPacket, InteractPacket,
    InteractPacketActionId, McpePacket, McpePacketData, MobEquipmentPacket, PlayerActionPacket,
    TextPacket, TextPacketType,
};

impl GameServer {
    /// Handle an inbound packet from a player.
    pub fn handle_packet(&mut self, session_id: SessionId, packet: McpePacket) {
        // Get player entity
        let entity = {
            let session_map = match self.ecs.world().get_resource::<SessionEntityMap>() {
                Some(map) => map,
                None => return,
            };
            match session_map.get(session_id) {
                Some(e) => e,
                None => return,
            }
        };

        // Special case: Disconnect packet (immediate handling)
        if matches!(&packet.data, McpePacketData::PacketDisconnect(_)) {
            info!(session_id, "Client sent disconnect packet");
            // Emit PlayerQuit event
            if let Some(mut event_buffer) = self
                .ecs
                .world_mut()
                .get_resource_mut::<crate::ecs::events::EventBuffer>()
            {
                event_buffer.push(crate::ecs::events::ServerEvent::PlayerQuit { entity });
            }
            self.despawn_player(session_id);
            return;
        }

        // Special case: Interact packet (needs special handling)
        if let McpePacketData::PacketInteract(pk) = &packet.data {
            self.handle_interact(entity, pk);
            return;
        }

        // Route packet to queues for future parallel processing
        if let Some(mut queues) = self
            .ecs
            .world_mut()
            .get_resource_mut::<super::packet_routing::PacketQueues>()
        {
            super::packet_router::PacketRouter::route_packet(
                session_id,
                entity,
                packet.clone(),
                &mut queues,
            );
        }

        // TEMPORARY: Also call old handlers directly until domain systems are built
        match &packet.data {
            McpePacketData::PacketRequestChunkRadius(req) => {
                self.handle_chunk_radius_request(entity, req);
            }
            McpePacketData::PacketCommandRequest(req) => {
                self.handle_command_request(session_id, req);
            }
            McpePacketData::PacketPlayerAuthInput(pk) => {
                self.handle_player_auth_input(entity, pk);
            }
            McpePacketData::PacketMovePlayer(_) => {
                // Intentionally ignored - use PlayerAuthInput instead
            }
            McpePacketData::PacketSubchunkRequest(req) => {
                self.handle_subchunk_request(entity, req);
            }
            McpePacketData::PacketMobEquipment(pk) => {
                self.handle_mob_equipment(entity, pk);
            }
            McpePacketData::PacketContainerClose(pk) => {
                self.handle_container_close(entity, pk);
            }
            McpePacketData::PacketItemStackRequest(pk) => {
                self.handle_item_stack_request(entity, pk);
            }
            McpePacketData::PacketInventoryTransaction(pk) => {
                self.handle_inventory_transaction(entity, pk);
            }
            McpePacketData::PacketText(pk) => {
                self.handle_text(session_id, entity, pk);
            }
            McpePacketData::PacketPlayerAction(pk) => {
                self.handle_player_action(entity, pk);
            }
            McpePacketData::PacketDisconnect(_) | McpePacketData::PacketInteract(_) => {
                // Already handled above
            }
            _ => {
                debug!(
                    session_id,
                    packet_id = ?packet.data.packet_id(),
                    "Unhandled packet"
                );
            }
        }
    }

    /// Handle player movement and input from PlayerAuthInput packet.
    pub(super) fn handle_player_auth_input(
        &mut self,
        entity: Entity,
        pk: &jolyne::valentine::PlayerAuthInputPacket,
    ) {
        // Update position and rotation
        let new_pos = DVec3::new(
            pk.position.x as f64,
            pk.position.y as f64,
            pk.position.z as f64,
        );

        let world = self.ecs.world_mut();

        // Update Position
        let old_pos = if let Some(mut position) = world.get_mut::<Position>(entity) {
            let old = position.0;
            position.0 = new_pos;
            old
        } else {
            new_pos
        };

        // Update Rotation
        if let Some(mut rotation) = world.get_mut::<Rotation>(entity) {
            rotation.pitch = pk.pitch;
            rotation.yaw = pk.yaw;
            rotation.head_yaw = pk.head_yaw;
        }

        // Update PlayerInput from InputFlag
        let mut jumped = false;
        if let Some(mut input) = world.get_mut::<PlayerInput>(entity) {
            input.move_x = pk.move_vector.x;
            input.move_z = pk.move_vector.z;

            let was_jumping = input.jumping;
            input.jumping = pk.input_data.contains(InputFlag::JUMPING)
                || pk.input_data.contains(InputFlag::START_JUMPING);

            if input.jumping && !was_jumping {
                jumped = true;
            }

            input.sneaking = pk.input_data.contains(InputFlag::SNEAKING);
            input.sprinting = pk.input_data.contains(InputFlag::SPRINTING);
            input.tick = pk.tick;
            // NOTE: on_ground is inferred from collision flags or physics; not sent directly
            input.on_ground = !pk.input_data.contains(InputFlag::VERTICAL_COLLISION);
        }

        // Emit events to PluginManager via EventBuffer
        if let Some(mut event_buffer) = world.get_resource_mut::<crate::ecs::events::EventBuffer>()
        {
            use crate::ecs::events::ServerEvent;

            // Emit Move event
            if old_pos.distance_squared(new_pos) > 1e-3 {
                event_buffer.push(ServerEvent::PlayerMove {
                    entity,
                    from: (old_pos.x, old_pos.y, old_pos.z),
                    to: (new_pos.x, new_pos.y, new_pos.z),
                });
            }

            // Emit Jump event
            if jumped {
                event_buffer.push(ServerEvent::PlayerJump { entity });
            }
        }

        // Update PlayerState persistent flags
        // Update PlayerState persistent flags
        let mut toggle_sneak = None;
        let mut toggle_sprint = None;

        if let Some(mut state) = world.get_mut::<PlayerState>(entity) {
            // Handle toggle events
            if pk.input_data.contains(InputFlag::START_SNEAKING) {
                state.sneaking = true;
                toggle_sneak = Some(true);
            }
            if pk.input_data.contains(InputFlag::STOP_SNEAKING) {
                state.sneaking = false;
                toggle_sneak = Some(false);
            }
            if pk.input_data.contains(InputFlag::START_SPRINTING) {
                state.sprinting = true;
                toggle_sprint = Some(true);
            }
            if pk.input_data.contains(InputFlag::STOP_SPRINTING) {
                state.sprinting = false;
                toggle_sprint = Some(false);
            }
            if pk.input_data.contains(InputFlag::START_SWIMMING) {
                state.swimming = true;
            }
            if pk.input_data.contains(InputFlag::STOP_SWIMMING) {
                state.swimming = false;
            }
            if pk.input_data.contains(InputFlag::START_GLIDING) {
                state.gliding = true;
            }
            if pk.input_data.contains(InputFlag::STOP_GLIDING) {
                state.gliding = false;
            }
            if pk.input_data.contains(InputFlag::START_FLYING) {
                state.flying = true;
            }
            if pk.input_data.contains(InputFlag::STOP_FLYING) {
                state.flying = false;
            }
        }

        // Emit toggle events after releasing PlayerState borrow
        if let Some(is_sneaking) = toggle_sneak {
            if let Some(mut event_buffer) =
                world.get_resource_mut::<crate::ecs::events::EventBuffer>()
            {
                event_buffer.push(crate::ecs::events::ServerEvent::PlayerToggleSneak {
                    entity,
                    is_sneaking,
                });
            }
        }
        if let Some(is_sprinting) = toggle_sprint {
            if let Some(mut event_buffer) =
                world.get_resource_mut::<crate::ecs::events::EventBuffer>()
            {
                event_buffer.push(crate::ecs::events::ServerEvent::PlayerToggleSprint {
                    entity,
                    is_sprinting,
                });
            }
        }

        // Handle block actions (breaking blocks)
        self.handle_block_actions(entity, pk);
    }

    /// Handle hotbar slot changes from MobEquipment packet.
    ///
    /// When a player scrolls their hotbar or presses 1-9, the client sends
    /// this packet to indicate which slot is now selected.
    pub(super) fn handle_mob_equipment(&mut self, entity: Entity, pk: &MobEquipmentPacket) {
        let world = self.ecs.world_mut();

        // Update HeldSlot to the newly selected slot
        // Update HeldSlot to the newly selected slot
        let mut slot_changed = None;

        if let Some(mut held_slot) = world.get_mut::<HeldSlot>(entity) {
            let old_slot = held_slot.0;
            if old_slot != pk.selected_slot {
                held_slot.set(pk.selected_slot);
                slot_changed = Some(old_slot);
                trace!(
                    entity = ?entity,
                    slot = pk.selected_slot,
                    "Player changed held slot"
                );
            }
        }

        if let Some(old_slot) = slot_changed {
            if let Some(mut event_buffer) =
                world.get_resource_mut::<crate::ecs::events::EventBuffer>()
            {
                event_buffer.push(crate::ecs::events::ServerEvent::PlayerHeldSlotChange {
                    entity,
                    old_slot,
                    new_slot: pk.selected_slot,
                });
            }
        }

        // TODO: Broadcast held item change to other players for rendering
    }

    /// Handle Interact packet (opening inventory, etc.)
    ///
    /// When the player presses E, the client sends this packet with OpenInventory action.
    /// We must respond with a ContainerOpen packet to actually open the inventory UI.
    pub(super) fn handle_interact(&mut self, entity: Entity, pk: &InteractPacket) {
        debug!(
            entity = ?entity,
            action = ?pk.action_id,
            "Received Interact packet"
        );

        match pk.action_id {
            InteractPacketActionId::OpenInventory => {
                // Check if inventory is already open to prevent duplicate ContainerOpen
                // which would crash the client
                {
                    let world = self.ecs.world();
                    if let Some(opened) = world.get::<InventoryOpened>(entity) {
                        if opened.0 {
                            debug!(entity = ?entity, "Inventory already open, skipping");
                            return;
                        }
                    }
                }

                debug!(entity = ?entity, "Player requested to open inventory");

                // Get player position and session for sending the response
                let world = self.ecs.world();
                let position = world
                    .get::<Position>(entity)
                    .map(|p| p.0)
                    .unwrap_or(DVec3::ZERO);
                let session = match world.get::<PlayerSession>(entity) {
                    Some(s) => s,
                    None => return,
                };

                // Send ContainerOpen packet to actually open the inventory UI
                // WindowID 0 = Inventory window
                // WindowType::Inventory (-1) = Player inventory container type
                let result = session.send(McpePacket::from(ContainerOpenPacket {
                    window_id: WindowId::Inventory,     // 0
                    window_type: WindowType::Inventory, // -1 (0xff in unsigned)
                    coordinates: BlockCoordinates {
                        x: position.x as i32,
                        y: position.y as i32,
                        z: position.z as i32,
                    },
                    runtime_entity_id: -1, // -1 for player's own inventory
                }));

                debug!("Sent ContainerOpen packet: {:?}", result);

                // Mark inventory as opened
                let world = self.ecs.world_mut();
                if let Some(mut opened) = world.get_mut::<InventoryOpened>(entity) {
                    opened.0 = true;
                }
            }
            InteractPacketActionId::MouseOverEntity => {
                // Ignored - client sends this when hovering over entities
            }
            _ => {
                debug!(entity = ?entity, action = ?pk.action_id, "Unhandled Interact action");
            }
        }
    }

    /// Handle ContainerClose packet.
    ///
    /// When the player presses E again (or Escape) to close their inventory,
    /// the client sends this packet. We must acknowledge it and update our state.
    pub(super) fn handle_container_close(&mut self, entity: Entity, pk: &ContainerClosePacket) {
        debug!(
            entity = ?entity,
            window_id = ?pk.window_id,
            "Received ContainerClose packet"
        );

        // Handle closing the player's inventory (WindowID 0)
        if pk.window_id == WindowId::Inventory {
            // Send ContainerClose back to acknowledge
            let world = self.ecs.world();
            if let Some(session) = world.get::<PlayerSession>(entity) {
                let _ = session.send(McpePacket::from(ContainerClosePacket {
                    window_id: WindowId::Inventory,
                    window_type: WindowType::Inventory,
                    server: false, // false = client initiated close
                }));
                debug!("Sent ContainerClose acknowledgement");
            }

            // Mark inventory as closed
            let world = self.ecs.world_mut();
            if let Some(mut opened) = world.get_mut::<InventoryOpened>(entity) {
                opened.0 = false;
            }
        } else {
            debug!(window_id = ?pk.window_id, "ContainerClose for non-inventory window (not handled)");
        }
    }

    /// Handle ItemStackRequest - creative inventory picks, item moves, etc.
    ///
    /// This handles the core inventory transactions from the client:
    /// - CraftCreative: Player clicked an item in creative inventory
    /// - Place: Player placed an item into a slot
    /// - Take: Player picked up an item
    /// - Destroy: Player deleted an item (creative mode)
    pub(super) fn handle_item_stack_request(
        &mut self,
        entity: Entity,
        pk: &jolyne::valentine::ItemStackRequestPacket,
    ) {
        use crate::entity::components::{ItemStackRequestState, MainInventory};
        use crate::item::ItemStack;
        use jolyne::valentine::ItemStackResponsePacket;
        use jolyne::valentine::types::FullContainerName;
        use jolyne::valentine::types::{
            ItemStackRequestActionsItemContent, ItemStackResponsesItem,
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

            // Track pending items from CraftCreative actions within this request
            let mut pending_item: Option<ItemStack> = None;
            let mut response_containers: Vec<ItemStackResponsesItemContentContainersItem> = vec![];

            for action in &request.actions {
                match &action.content {
                    Some(ItemStackRequestActionsItemContent::CraftCreative(craft)) => {
                        // Look up creative item by 1-indexed network ID
                        let index = (craft.item_id - 1) as usize;

                        info!(
                            "CraftCreative: client requested item_id={}, index={}, total_items={}",
                            craft.item_id,
                            index,
                            self.world_template.creative_content.items.len()
                        );

                        // Get creative content from GameServer
                        if let Some(entry) = self.world_template.creative_content.items.get(index) {
                            // Resolve proper item string ID from ItemRegistry
                            let item_id = if let Some(registry) =
                                self.ecs
                                    .world()
                                    .get_resource::<super::types::ItemRegistryResource>()
                            {
                                registry
                                    .0
                                    .get(entry.item.network_id as u32)
                                    .map(|e| e.string_id.clone())
                                    .unwrap_or_else(|| {
                                        warn!(
                                            "Creative item network_id {} not found in registry",
                                            entry.item.network_id
                                        );
                                        format!("minecraft:network_{}", entry.item.network_id)
                                    })
                            } else {
                                format!("minecraft:network_{}", entry.item.network_id)
                            };

                            // Convert to ItemStack
                            let item = ItemStack::new(
                                item_id.clone(),
                                64, // Max stack in creative
                            );
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
                        // Place pending item into destination slot
                        if let Some(item) = pending_item.take() {
                            let dest_slot = place.destination.slot as usize;
                            let dest_container = place.destination.slot_type.container_id;

                            // Get new stack ID
                            let stack_id = {
                                let world = self.ecs.world_mut();
                                if let Some(mut state) =
                                    world.get_mut::<ItemStackRequestState>(entity)
                                {
                                    state.next_id()
                                } else {
                                    1
                                }
                            };

                            // Update inventory based on container type
                            if dest_container == ContainerSlotType::HotbarAndInventory {
                                let world = self.ecs.world_mut();
                                if let Some(mut inv) = world.get_mut::<MainInventory>(entity) {
                                    let _ = inv.0.set_item(dest_slot, item.clone());
                                    info!(
                                        slot = dest_slot,
                                        item_id = %item.item_id,
                                        stack_id,
                                        "Placed item in inventory"
                                    );
                                }
                            }

                            // Build response slot info
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
                        info!(
                            source_slot = take.source.slot,
                            dest_slot = take.destination.slot,
                            count = take.count,
                            "Take action details"
                        );
                    }
                    Some(ItemStackRequestActionsItemContent::Destroy(_destroy)) => {
                        // In creative mode, just acknowledge - item is deleted
                        debug!("Destroy action - item deleted");
                    }
                    _ => {
                        trace!(type_id = ?action.type_id, "Unhandled ItemStackRequest action type");
                    }
                }
            }

            // Send response
            let world = self.ecs.world();
            if let Some(session) = world.get::<crate::entity::components::PlayerSession>(entity) {
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

    /// Handle inventory transaction packets (used for creative inventory, item drops, etc.)
    fn handle_inventory_transaction(
        &mut self,
        entity: Entity,
        pk: &jolyne::valentine::InventoryTransactionPacket,
    ) {
        use jolyne::valentine::types::TransactionTransactionType;

        let transaction = &pk.transaction;

        info!(
            transaction_type = ?transaction.transaction_type,
            actions = transaction.actions.len(),
            "Received InventoryTransaction"
        );

        match transaction.transaction_type {
            TransactionTransactionType::Normal => {
                // Normal transactions include creative inventory picks
                self.handle_normal_transaction(entity, &transaction.actions);
            }
            TransactionTransactionType::InventoryMismatch => {
                // Client and server inventories are out of sync - resend
                debug!("Inventory mismatch - should resend inventory");
            }
            TransactionTransactionType::ItemUse => {
                // Item use (clicking blocks, etc.)
                if let Some(data) = &transaction.transaction_data {
                    use jolyne::valentine::types::TransactionTransactionData;
                    if let TransactionTransactionData::ItemUse(use_item) = data {
                        use jolyne::valentine::types::TransactionUseItemActionType;
                        match use_item.action_type {
                            TransactionUseItemActionType::ClickBlock => {
                                self.handle_block_click(entity, use_item);
                            }
                            TransactionUseItemActionType::ClickAir => {
                                // Emit PlayerItemUse event
                                if let Some(mut event_buffer) =
                                    self.ecs
                                        .world_mut()
                                        .get_resource_mut::<crate::ecs::events::EventBuffer>()
                                {
                                    event_buffer.push(
                                        crate::ecs::events::ServerEvent::PlayerItemUse { entity },
                                    );
                                }
                            }
                            _ => {
                                debug!(action = ?use_item.action_type, "Unhandled ItemUse action");
                            }
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
        }
    }

    /// Handle Normal inventory transactions (creative inventory picks, drops)
    fn handle_normal_transaction(
        &mut self,
        entity: Entity,
        actions: &[jolyne::valentine::types::TransactionActionsItem],
    ) {
        use jolyne::valentine::types::TransactionActionsItemSourceType;

        for action in actions {
            match action.source_type {
                TransactionActionsItemSourceType::Creative => {
                    // Creative inventory action - client is taking an item from creative
                    // new_item contains the item being taken
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
                TransactionActionsItemSourceType::WorldInteraction => {
                    use jolyne::valentine::types::TransactionActionsItemContent;
                    // World interaction (Item Use / Interact Block)
                    // The flags in transaction_data.action_type might tell us more, but
                    // TransactionActionsItemSourceType::WorldInteraction usually accompanies item use.

                    // We need to look at the transaction type from the packet, but here we only have actions list.
                    // Actually, pk is passed to handle_inventory_transaction, but we are inside the loop over actions.
                    // The packet structure has transaction_data.
                }
                TransactionActionsItemSourceType::Container => {
                    // Container action - placing into inventory slot
                    if let Some(content) = &action.content {
                        use jolyne::valentine::types::TransactionActionsItemContent;
                        if let TransactionActionsItemContent::Container(container) = content {
                            let slot = action.slot as usize;

                            // Check if new_item is being added (not air)
                            if action.new_item.network_id != 0 {
                                // Get count from content (defaults to 1)
                                let count = action
                                    .new_item
                                    .content
                                    .as_ref()
                                    .map(|c| c.count as u8)
                                    .unwrap_or(1);

                                // Resolve Item ID
                                let item_id = if let Some(registry) =
                                    self.ecs
                                        .world()
                                        .get_resource::<super::types::ItemRegistryResource>()
                                {
                                    registry
                                        .0
                                        .get(action.new_item.network_id as u32)
                                        .map(|e| e.string_id.clone())
                                        .unwrap_or_else(|| {
                                            format!(
                                                "minecraft:network_{}",
                                                action.new_item.network_id
                                            )
                                        })
                                } else {
                                    format!("minecraft:network_{}", action.new_item.network_id)
                                };

                                // Create ItemStack from the protocol item
                                let item = crate::item::ItemStack::new(item_id.clone(), count);
                                info!(?item_id, count, "Created item from creative action");

                                // Update player inventory
                                let world = self.ecs.world_mut();
                                if let Some(mut inv) = world
                                    .get_mut::<crate::entity::components::MainInventory>(entity)
                                {
                                    let _ = inv.0.set_item(slot, item.clone());
                                    info!(
                                        slot,
                                        network_id = action.new_item.network_id,
                                        count,
                                        window = ?container.inventory_id,
                                        "Placed item in inventory from creative"
                                    );
                                }
                            }
                            // Note: We don't clear slots here as creative transactions
                            // are for adding items, not removing them
                        }
                    }
                }
                TransactionActionsItemSourceType::WorldInteraction => {
                    // Dropping item into world
                    debug!(slot = action.slot, "World interaction (item drop)");
                }
                _ => {
                    debug!(source_type = ?action.source_type, "Unhandled action source type");
                }
            }
        }
    }

    /// Handle Animate packet (arm swings, critical hits, etc.).
    ///
    /// When a player swings their arm or performs other animations,
    /// we need to broadcast this to nearby players so they can see it.
    pub(super) fn handle_animate(
        &mut self,
        session_id: SessionId,
        entity: Entity,
        pk: &AnimatePacket,
    ) {
        use crate::entity::components::RuntimeEntityId;

        debug!(
            session_id,
            action = ?pk.action_id,
            runtime_id = pk.runtime_entity_id,
            "Received Animate packet"
        );

        use jolyne::valentine::types::AnimatePacketActionId;
        if pk.action_id == AnimatePacketActionId::SwingArm {
            if let Some(mut event_buffer) = self
                .ecs
                .world_mut()
                .get_resource_mut::<crate::ecs::events::EventBuffer>()
            {
                event_buffer.push(crate::ecs::events::ServerEvent::PlayerSwing { entity });
            }
        }

        // Get the sender's runtime ID for broadcasting
        let world = self.ecs.world();
        let sender_runtime_id = world
            .get::<RuntimeEntityId>(entity)
            .map(|r| r.0)
            .unwrap_or(pk.runtime_entity_id);

        // Build the animate packet to broadcast to other players
        let broadcast_packet = AnimatePacket {
            action_id: pk.action_id,
            runtime_entity_id: sender_runtime_id,
            data: pk.data,
            swing_source: pk.swing_source.clone(),
        };

        // Broadcast to all other players
        let world = self.ecs.world();
        let session_map = match world.get_resource::<SessionEntityMap>() {
            Some(map) => map,
            None => return,
        };

        for (_sid, other_entity) in session_map.iter() {
            if other_entity == entity {
                continue; // Skip sender
            }
            if let Some(other_session) = world.get::<PlayerSession>(other_entity) {
                let _ = other_session.send(McpePacket::from(broadcast_packet.clone()));
            }
        }
    }

    /// Handle Text packet (chat messages, whispers, etc.).
    ///
    /// When a player sends a chat message, we need to format it and
    /// broadcast it to the appropriate recipients.
    pub(super) fn handle_text(&mut self, session_id: SessionId, entity: Entity, pk: &TextPacket) {
        use crate::ecs::events::EventBuffer;
        use crate::entity::components::PlayerName;
        use crate::entity::components::PlayerUuid;

        debug!(
            session_id,
            text_type = ?pk.type_,
            source = %pk.source_name,
            message = %pk.message,
            "Received Text packet"
        );

        // The message is now directly available on the packet
        let message = pk.message.clone();
        if message.is_empty() {
            trace!("Text packet with empty message");
            return;
        }

        // Get sender's name and UUID
        let world = self.ecs.world();
        let sender_name = world
            .get::<PlayerName>(entity)
            .map(|n| n.0.clone())
            .unwrap_or_else(|| "Unknown".to_string());
        let sender_uuid = world
            .get::<PlayerUuid>(entity)
            .map(|u| u.0.to_string())
            .unwrap_or_default();

        match pk.type_ {
            TextPacketType::Chat => {
                info!(
                    sender = %sender_name,
                    message = %message,
                    "Chat message"
                );

                // Call plugin chat handlers
                // Call plugin chat handlers
                {
                    // Remove registry to avoid borrow conflict with World
                    if let Some(mut registry) = self
                        .ecs
                        .world_mut()
                        .remove_resource::<crate::plugin::PluginRegistry>()
                    {
                        // Extract component data once
                        let (name, position, uuid) = {
                            let world = self.ecs.world();
                            let n = world
                                .get::<crate::entity::components::PlayerName>(entity)
                                .map(|c| c.0.clone());
                            let p = world
                                .get::<crate::entity::components::Position>(entity)
                                .map(|c| unastar_api::native::Vec3 {
                                    x: c.0.x,
                                    y: c.0.y,
                                    z: c.0.z,
                                });
                            let u = world
                                .get::<crate::entity::components::PlayerUuid>(entity)
                                .map(|c| c.0.to_string());
                            (n, p, u)
                        };

                        let allow = {
                            let host = crate::server::game::host::ServerHost {
                                world: self.ecs.world_mut(),
                            };

                            let mut player = unastar_api::native::Player::new(
                                unastar_api::native::PluginEntity::from(entity),
                                unastar_api::native::RawPluginHost_TO::from_value(
                                    host,
                                    abi_stable::sabi_trait::TD_Opaque,
                                ),
                                name.clone(),
                                position.clone(),
                                uuid.clone(),
                            );
                            registry.on_chat(&mut player, &message)
                        };

                        self.ecs.world_mut().insert_resource(registry);

                        if !allow {
                            debug!("Chat message canceled by plugin");
                            return;
                        }
                    }
                }

                // Emit PlayerChat event to plugins
                if let Some(mut event_buffer) =
                    self.ecs.world_mut().get_resource_mut::<EventBuffer>()
                {
                    event_buffer.push(crate::ecs::events::ServerEvent::PlayerChat {
                        entity,
                        player_id: sender_uuid.clone(),
                        message: message.clone(),
                    });
                }

                // Broadcast chat to all players
                let broadcast_packet = TextPacket {
                    type_: TextPacketType::Chat,
                    needs_translation: false,
                    source_name: sender_name.clone(),
                    message: message.clone(),
                    parameters: Vec::new(),
                    xuid: String::new(),
                    platform_chat_id: String::new(),
                    filtered_message: None,
                };

                let world = self.ecs.world();
                let session_map = match world.get_resource::<SessionEntityMap>() {
                    Some(map) => map,
                    None => return,
                };

                for (_sid, other_entity) in session_map.iter() {
                    if let Some(other_session) = world.get::<PlayerSession>(other_entity) {
                        let _ = other_session.send(McpePacket::from(broadcast_packet.clone()));
                    }
                }
            }
            TextPacketType::Whisper => {
                // For whisper, we'd need to find the target player and send privately
                // This requires extracting the target from the message (e.g., "/tell Player msg")
                debug!(
                    sender = %sender_name,
                    message = %message,
                    "Whisper message (not fully implemented)"
                );
            }
            _ => {
                trace!(text_type = ?pk.type_, "Unhandled text type");
            }
        }
    }

    /// Handle PlayerAction packet (jump, sprint, sneak, respawn, etc.).
    ///
    /// This packet is sent when a player performs various actions that
    /// aren't covered by PlayerAuthInput (which handles most movement).
    pub(super) fn handle_player_action(&mut self, entity: Entity, pk: &PlayerActionPacket) {
        use crate::entity::components::PlayerState;

        trace!(
            action = ?pk.action,
            position = ?pk.position,
            face = pk.face,
            "Received PlayerAction packet"
        );

        let world = self.ecs.world_mut();

        match pk.action {
            Action::Jump => {
                // Jump action - mostly informational, physics handled elsewhere
                trace!("Player jumped");
            }
            Action::StartBreak => {
                if let Some(mut event_buffer) =
                    world.get_resource_mut::<crate::ecs::events::EventBuffer>()
                {
                    event_buffer.push(crate::ecs::events::ServerEvent::PlayerStartBreak {
                        entity,
                        position: (pk.position.x, pk.position.y, pk.position.z),
                        face: pk.face as u8,
                    });
                }
            }
            Action::StartSprint => {
                if let Some(mut state) = world.get_mut::<PlayerState>(entity) {
                    state.sprinting = true;
                }
            }
            Action::StopSprint => {
                if let Some(mut state) = world.get_mut::<PlayerState>(entity) {
                    state.sprinting = false;
                }
            }
            Action::StartSneak => {
                if let Some(mut state) = world.get_mut::<PlayerState>(entity) {
                    state.sneaking = true;
                }
            }
            Action::StopSneak => {
                if let Some(mut state) = world.get_mut::<PlayerState>(entity) {
                    state.sneaking = false;
                }
            }
            Action::Respawn => {
                debug!("Player requested respawn");
                // TODO: Implement respawn logic - reset position, health, etc.
            }
            Action::DimensionChangeAck => {
                debug!("Player acknowledged dimension change");
                // Client is ready after dimension change
            }
            Action::HandledTeleport => {
                trace!("Player handled teleport");
            }
            _ => {
                trace!(action = ?pk.action, "Unhandled player action");
            }
        }
    }
}
