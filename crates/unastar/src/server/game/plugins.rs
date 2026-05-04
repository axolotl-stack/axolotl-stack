use crate::ecs::events::{ActionQueue, PluginAction};
use crate::entity::components::{PlayerSession, Position, Rotation, RuntimeEntityId};
use crate::server::game::types::system_text;
use crate::world::ecs::{
    BlockBroadcastEvent, ChunkData, ChunkManager, ChunkStateFlags, world_to_chunk_coords,
    world_to_local_coords,
};
use bevy_ecs::prelude::*;
use glam::DVec3;
use jolyne::valentine::{
    DisconnectFailReason, DisconnectPacket, DisconnectPacketContent, LegacyEntityType, McpePacket,
    MovePlayerPacket, MovePlayerPacketMode, MovePlayerPacketTeleport,
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
    chunk_manager: Res<ChunkManager>,
    mut chunks: Query<(&mut ChunkData, &mut ChunkStateFlags)>,
    mut block_events: ResMut<bevy_ecs::message::Messages<BlockBroadcastEvent>>,
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
                    let item_stack = if let Some(entry) = item_registry.0.get_by_name(&item_id) {
                        ItemStack::new(item_id.clone(), count).with_max_stack_size(entry.stack_size)
                    } else {
                        ItemStack::new(item_id.clone(), count)
                    };

                    if let Some(empty_slot) =
                        (0..36).find(|&i| inv.0.item(i).is_none_or(|item| item.is_empty()))
                    {
                        let _ = inv.0.set_item(empty_slot, item_stack.clone());
                        let effective_count = item_stack.count;
                        info!(entity=?entity, item=%item_id, count = effective_count, requested_count = count, slot=empty_slot, "Plugin gave item");

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
                                count: effective_count as u16,
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

                        let msg = format!("§aReceived {} x{}", item_id, effective_count);
                        let packet = system_text(&msg);
                        let _ = session.send(McpePacket::from(packet));
                    } else {
                        warn!(entity=?entity, "Inventory full, cannot give item");
                    }
                }
            }
            PluginAction::Kick { entity, reason } => {
                if let Ok((_, _, _, session, _)) = players.get(entity) {
                    let packet = DisconnectPacket {
                        reason: DisconnectFailReason::Disconnected,
                        hide_disconnect_reason: false,
                        content: Some(DisconnectPacketContent {
                            message: reason.clone(),
                            filtered_message: reason.clone(),
                        }),
                    };
                    let _ = session.send(McpePacket::from(packet));
                    info!(entity=?entity, reason=%reason, "Plugin kicked player");
                } else {
                    warn!(entity=?entity, reason=%reason, "Plugin kick target not found");
                }
            }
            PluginAction::SetBlock { position, block_id } => {
                let (x, y, z) = position;
                let (cx, cz) = world_to_chunk_coords(x, z);
                let Some(chunk_entity) = chunk_manager.get_by_coords(cx, cz) else {
                    warn!(pos=?position, block_id, "Plugin set_block target chunk is not loaded");
                    continue;
                };

                let (local_x, local_y, local_z) = world_to_local_coords(x, y, z);
                if let Ok((mut chunk_data, mut state_flags)) = chunks.get_mut(chunk_entity) {
                    chunk_data
                        .inner
                        .set_block(local_x, local_y, local_z, block_id);
                    state_flags.mark_dirty();
                    state_flags.mark_needs_rebroadcast();
                    block_events.write(BlockBroadcastEvent {
                        chunk_entity,
                        block_pos: glam::IVec3::new(x, y, z),
                        new_block: block_id,
                    });
                    info!(pos=?position, block_id, "Plugin set block");
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ecs::events::PluginAction;
    use crate::entity::components::{MainInventory, PlayerSession, Rotation, RuntimeEntityId};
    use crate::registry::block::BlockRegistry;
    use crate::registry::item::{ItemEntry, ItemRegistry};
    use crate::server::game::types::{BlockRegistryResource, ItemRegistryResource};
    use crate::world::{WorldConfig, ecs::ChunkManager};
    use jolyne::valentine::McpePacketData;
    use std::sync::Arc;
    use tokio::sync::mpsc;

    #[test]
    fn give_item_uses_effective_clamped_count_in_inventory_and_packet() {
        let mut world = World::new();
        world.insert_resource(ActionQueue::default());
        world.insert_resource(ItemRegistryResource(Arc::new(limited_item_registry())));
        world.insert_resource(BlockRegistryResource(Arc::new(BlockRegistry::new())));
        world.insert_resource(ChunkManager::new(WorldConfig::default()));
        world.insert_resource(bevy_ecs::message::Messages::<BlockBroadcastEvent>::default());

        let (outbound_tx, mut outbound_rx) = mpsc::channel(8);
        let player = world
            .spawn((
                Position(DVec3::ZERO),
                Rotation::default(),
                RuntimeEntityId(1),
                PlayerSession::new(1, "test".to_string(), None, None, outbound_tx),
                MainInventory::default(),
            ))
            .id();
        world
            .resource_mut::<ActionQueue>()
            .push(PluginAction::GiveItem {
                entity: player,
                item_id: "minecraft:honey_bottle".to_string(),
                count: 64,
            });

        let mut schedule = Schedule::default();
        schedule.add_systems(process_plugin_actions);
        schedule.run(&mut world);

        let inventory = world.get::<MainInventory>(player).expect("main inventory");
        let stack = inventory.0.item(0).expect("slot 0 stack");
        assert_eq!(stack.item_id, "minecraft:honey_bottle");
        assert_eq!(stack.count, 16);
        assert_eq!(stack.max_stack_size(), 16);

        let packet = outbound_rx.try_recv().expect("inventory slot packet");
        let McpePacketData::PacketInventorySlot(slot_packet) = packet.data else {
            panic!("expected inventory slot packet");
        };
        let content = slot_packet
            .item
            .content
            .as_ref()
            .expect("packet item content");
        assert_eq!(content.count, 16);
    }

    fn limited_item_registry() -> ItemRegistry {
        let mut registry = ItemRegistry::new();
        registry
            .register(ItemEntry {
                id: 1,
                network_id: 77,
                component_based: false,
                version: 0,
                string_id: "minecraft:honey_bottle".to_string(),
                name: "honey bottle".to_string(),
                stack_size: 16,
            })
            .expect("register limited item");
        registry
    }
}
