use crate::ecs::events::{ActionQueue, PluginAction};
use crate::entity::components::{PlayerSession, PlayerUuid, Position, Rotation, RuntimeEntityId};
use crate::server::game::types::system_text;
use bevy_ecs::prelude::*;
use glam::DVec3;
use jolyne::valentine::{
    LegacyEntityType, McpePacket, MovePlayerPacket, MovePlayerPacketMode, MovePlayerPacketTeleport,
    MovePlayerPacketTeleportCause, Vec3F,
};
use tracing::{info, warn};

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
                for (mut player_pos, rot, rid, uuid, session, _) in players.iter_mut() {
                    if uuid.0.to_string() == player_id {
                        let new_pos = DVec3::new(pos.0, pos.1, pos.2);
                        player_pos.0 = new_pos;

                        let packet = MovePlayerPacket {
                            runtime_id: rid.0 as i32,
                            position: Vec3F {
                                x: pos.0 as f32,
                                y: pos.1 as f32,
                                z: pos.2 as f32,
                            },
                            pitch: rot.pitch,
                            yaw: rot.yaw,
                            head_yaw: rot.yaw,
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
                        let item_stack = ItemStack::new(item_id.clone(), count);

                        if let Some(empty_slot) =
                            (0..36).find(|&i| inv.0.item(i).is_none_or(|item| item.is_empty()))
                        {
                            let _ = inv.0.set_item(empty_slot, item_stack.clone());
                            info!(player=%player_id, item=%item_id, count, slot=empty_slot, "Plugin gave item");

                            let network_id = item_registry
                                .0
                                .get_by_name(&item_id)
                                .map(|entry| entry.id as i32)
                                .unwrap_or_else(|| {
                                    warn!(
                                        "Item {} not found in registry, using placeholder",
                                        item_id
                                    );
                                    1
                                });

                            let block_runtime_id =
                                if let Some(entry) = block_registry.0.get_by_name(&item_id) {
                                    entry.min_state_id as i32
                                } else {
                                    0
                                };

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
            PluginAction::Kick { player_id, reason } => {
                for (_, _, _, uuid, _session, _) in players.iter() {
                    if uuid.0.to_string() == player_id {
                        // TODO: Send DisconnectPacket
                        warn!(player=%player_id, reason=%reason, "Plugin kick requested (not impl)");
                        break;
                    }
                }
            }
            PluginAction::SetBlock { position, block_id } => {
                // TODO: Implement block placement via ChunkManager
                info!(pos=?position, block_id, "Plugin set block (not yet impl)");
            }
        }
    }
}
