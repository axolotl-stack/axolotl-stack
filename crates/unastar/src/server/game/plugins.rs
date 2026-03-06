use crate::ecs::events::{ActionQueue, PluginAction};
use crate::entity::components::{PlayerSession, Position, Rotation, RuntimeEntityId};
use crate::server::game::types::system_text;
use bevy_ecs::prelude::*;
use glam::DVec3;
use jolyne::valentine::{
    LegacyEntityType, McpePacket, MovePlayerPacket, MovePlayerPacketMode, MovePlayerPacketTeleport,
    MovePlayerPacketTeleportCause, Vec3F,
};
use tracing::{info, warn};

/// System to handle actions requested by plugins via API.
///
/// All lookups are O(1) via direct entity access — no string-based scanning.
pub fn process_plugin_actions(
    mut action_queue: ResMut<ActionQueue>,
    item_registry: Res<super::types::ItemRegistryResource>,
    block_registry: Res<super::types::BlockRegistryResource>,
    mut players: Query<(
        &mut Position,
        &Rotation,
        &RuntimeEntityId,
        &PlayerSession,
        &mut crate::entity::components::MainInventory,
    )>,
) {
    let actions = action_queue.drain();
    let _span = tracing::info_span!("process_plugin_actions", count = actions.len()).entered();
    for action in actions {
        info!("Processing plugin action: {:?}", action);
        match action {
            PluginAction::SendMessage { entity, message } => {
                if let Ok((_, _, _, session, _)) = players.get(entity) {
                    let packet = system_text(&message);
                    let _ = session.send(McpePacket::from(packet));
                }
            }
            PluginAction::Teleport {
                entity,
                position: pos,
            } => {
                if let Ok((mut player_pos, rot, rid, session, _)) = players.get_mut(entity) {
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

                    info!(entity=?entity, to=?new_pos, "Plugin teleported player");
                }
            }
            PluginAction::GiveItem {
                entity,
                item_id,
                count,
            } => {
                use crate::item::ItemStack;
                use jolyne::valentine::types::{
                    ContainerSlotType, FullContainerName, Item, ItemContent, ItemContentExtra,
                };
                use jolyne::valentine::{InventorySlotPacket, WindowIdVarint};

                if let Ok((_, _, _, session, mut inv)) = players.get_mut(entity) {
                    let item_stack = ItemStack::new(item_id.clone(), count);

                    if let Some(empty_slot) =
                        (0..36).find(|&i| inv.0.item(i).is_none_or(|item| item.is_empty()))
                    {
                        let _ = inv.0.set_item(empty_slot, item_stack.clone());
                        info!(entity=?entity, item=%item_id, count, slot=empty_slot, "Plugin gave item");

                        let network_id = item_registry
                            .0
                            .get_by_name(&item_id)
                            .map(|entry| entry.network_id)
                            .unwrap_or_else(|| {
                                warn!("Item {} not found in registry, using placeholder", item_id);
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
                        warn!(entity=?entity, "Inventory full, cannot give item");
                    }
                }
            }
            PluginAction::Kick { entity, reason } => {
                warn!(entity=?entity, reason=%reason, "Plugin kick requested (not impl)");
            }
            PluginAction::SetBlock { position, block_id } => {
                info!(pos=?position, block_id, "Plugin set block (not yet impl)");
            }
        }
    }
}
