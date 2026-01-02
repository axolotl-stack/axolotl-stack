use crate::ecs::events::ActionQueue;
use crate::entity::components::{PlayerSession, PlayerUuid, Position, Rotation, RuntimeEntityId};
use crate::server::game::types::system_text;
use bevy_ecs::prelude::*;
use glam::DVec3;
use jolyne::valentine::{
    LegacyEntityType, McpePacket, MovePlayerPacket, MovePlayerPacketMode, MovePlayerPacketTeleport,
    MovePlayerPacketTeleportCause, Vec3F,
};
use tracing::{info, warn};
use unastar_api::PluginAction;

/// System to handle actions requested by plugins via API.
pub fn process_plugin_actions(
    mut action_queue: ResMut<ActionQueue>,
    item_registry: Res<super::types::ItemRegistryResource>,
    block_registry: Res<super::types::BlockRegistryResource>,
    mut players: Query<(
        &mut Position,
        &mut Rotation,
        &RuntimeEntityId,
        &PlayerUuid,
        &PlayerSession,
        &mut crate::entity::components::MainInventory,
    )>,
) {
    for action in action_queue.drain() {
        info!("Processing plugin action: {:?}", action);
        match action {
            PluginAction::SendMessage { player_id, message } => {
                for (_, _, _, uuid, session, _) in players.iter() {
                    if uuid.0.to_string() == player_id {
                        let packet = system_text(&message);
                        let _ = session.send(McpePacket::from(packet));
                        break;
                    }
                }
            }
            PluginAction::Teleport {
                player_id,
                position: pos,
            } => {
                for (mut player_pos, mut rot, rid, uuid, session, _) in players.iter_mut() {
                    info!(
                        "Checking player {} against target {}",
                        uuid.0.to_string(),
                        player_id
                    );
                    if uuid.0.to_string() == player_id {
                        let new_pos = DVec3::new(pos.x, pos.y, pos.z);
                        player_pos.0 = new_pos;

                        // Send Teleport Packet
                        let packet = MovePlayerPacket {
                            runtime_id: rid.0 as i32,
                            position: Vec3F {
                                x: pos.x as f32,
                                y: pos.y as f32,
                                z: pos.z as f32,
                            },
                            pitch: rot.pitch,
                            yaw: rot.yaw,
                            head_yaw: rot.yaw, // Use same yaw
                            mode: MovePlayerPacketMode::Teleport,
                            on_ground: false,
                            ridden_runtime_id: 0,
                            teleport: Some(MovePlayerPacketTeleport {
                                cause: MovePlayerPacketTeleportCause::Command,
                                source_entity_type: LegacyEntityType::Player,
                            }),
                            tick: 0,
                        };
                        let _ = session.send(McpePacket::from(packet));

                        info!(player=%player_id, to=?new_pos, "Plugin teleported player");
                        break;
                    }
                }
            }
            PluginAction::GiveItem {
                player_id,
                item_id,
                count,
            } => {
                use crate::item::ItemStack;
                use jolyne::valentine::types::{
                    ContainerSlotType, FullContainerName, Item, ItemContent, ItemContentExtra,
                };
                use jolyne::valentine::{InventorySlotPacket, WindowIdVarint};

                for (_, _, _, uuid, session, mut inv) in players.iter_mut() {
                    if uuid.0.to_string() == player_id {
                        // Create item stack
                        let item_stack = ItemStack::new(item_id.clone(), count);

                        // Try to add to inventory (find first empty slot)
                        if let Some(empty_slot) =
                            (0..36).find(|&i| inv.0.item(i).map_or(true, |item| item.is_empty()))
                        {
                            let _ = inv.0.set_item(empty_slot, item_stack.clone());
                            info!(player=%player_id, item=%item_id, count, slot=empty_slot, "Plugin gave item");

                            // Look up network_id from ItemRegistry
                            let network_id = item_registry
                                .0
                                .get_by_name(&item_id)
                                .map(|entry| entry.id as i32)
                                .unwrap_or_else(|| {
                                    warn!(
                                        "Item {} not found in registry, using placeholder",
                                        item_id
                                    );
                                    1 // fallback to dirt
                                });

                            // Look up block_runtime_id if it's a block item
                            // Try to get block by same name (e.g., minecraft:gold_block exists as both item and block)
                            // Use min_state_id as the runtime ID for the block
                            let block_runtime_id = if let Some(entry) =
                                block_registry.0.get_by_name(&item_id)
                            {
                                info!(
                                    "Found block {} with min_state_id {}",
                                    item_id, entry.min_state_id
                                );
                                entry.min_state_id as i32
                            } else {
                                warn!(
                                    "Block {} not found in BlockRegistry, item will not be placeable",
                                    item_id
                                );
                                0 // 0 = not a block item
                            };

                            // Create protocol Item to send to client
                            let protocol_item = Item {
                                network_id,
                                content: Some(Box::new(ItemContent {
                                    count: count as u16,
                                    metadata: 0,
                                    has_stack_id: 0,
                                    stack_id: None,
                                    block_runtime_id,
                                    extra: ItemContentExtra::Default(Default::default()),
                                })),
                            };

                            info!(
                                "Sending Item with network_id={}, block_runtime_id={}",
                                network_id, block_runtime_id
                            );
                            // Send InventorySlotPacket to sync with client
                            let slot_packet = InventorySlotPacket {
                                window_id: WindowIdVarint::Inventory,
                                slot: empty_slot as i32,
                                container: FullContainerName {
                                    container_id: ContainerSlotType::HotbarAndInventory,
                                    dynamic_container_id: None,
                                },
                                storage_item: protocol_item.clone(),
                                item: protocol_item,
                            };
                            let _ = session.send(McpePacket::from(slot_packet));

                            // Send message to player
                            let msg = format!("§aReceived {} x{}", item_id, count);
                            let packet = system_text(&msg);
                            let _ = session.send(McpePacket::from(packet));
                        } else {
                            warn!(player=%player_id, "Inventory full, cannot give item");
                        }
                        break;
                    }
                }
            }
            PluginAction::Log { .. } => {
                // Handled in PluginManager immediately, shouldn't be here
            }
            PluginAction::Kick { player_id, reason } => {
                for (_, _, _, uuid, _session, _) in players.iter() {
                    if uuid.0.to_string() == player_id {
                        // TODO: Send DisconnectPacket
                        warn!(player=%player_id, reason=%reason, "Plugin kick requested (not impl)");
                        break;
                    }
                }
            }
            PluginAction::Cancel { .. } => {
                // Handled in PluginManager
            }
        }
    }
}
