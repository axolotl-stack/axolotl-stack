//! Block domain system (exclusive).
//!
//! Processes block actions from PlayerAuthInput (break/place/crack) and
//! block clicks from InventoryTransaction. Runs as an exclusive system
//! because it needs `world.trigger()` and `world.write_message()`.

use bevy_ecs::prelude::*;
use glam::IVec3;
use tracing::{debug, info, trace, warn};

use super::packet_queues::{BlockAction, BlockPacketQueue};
use crate::ecs::events::{EventBuffer, ServerEvent};
use crate::entity::components::{BreakingState, PlayerSession, PlayerUuid};
use crate::world::chunk::blocks;
use crate::world::ecs::{BlockBroadcastEvent, BlockChanged, ChunkManager, ChunkViewers};
use crate::world::ecs::{world_to_chunk_coords, world_to_local_coords};
use jolyne::valentine::types::{Action, BlockCoordinates, Vec3F};
use jolyne::valentine::{LevelEventPacket, LevelEventPacketEvent, McpePacket};

/// Maximum block actions per PlayerAuthInput packet.
const MAX_BLOCK_ACTIONS: usize = 64;

/// ECS exclusive system: drain block packet queue and apply block changes.
///
/// Runs in `PacketApplySet` after `apply_movement` (which forwards block
/// actions from PlayerAuthInput). Exclusive because it fires observers
/// (`BlockChanged`) and writes messages (`BlockBroadcastEvent`).
pub fn apply_block_actions(world: &mut World) {
    let actions: Vec<_> = {
        let Some(mut queue) = world.get_resource_mut::<BlockPacketQueue>() else {
            return;
        };
        queue.0.drain(..).collect()
    };

    let _span = tracing::info_span!("apply_block_actions", count = actions.len()).entered();
    for (entity, action) in actions {
        match action {
            BlockAction::AuthInputActions(block_actions) => {
                process_auth_input_block_actions(world, entity, &block_actions);
            }
            BlockAction::BlockClick(use_item) => {
                handle_block_click(world, entity, &use_item);
            }
        }
    }
}

/// Process block actions from a PlayerAuthInput packet.
fn process_auth_input_block_actions(
    world: &mut World,
    player_entity: Entity,
    block_actions: &[jolyne::valentine::PlayerAuthInputPacketBlockActionItem],
) {
    let current_tick = world
        .get_resource::<crate::ecs::resources::TickCounter>()
        .unwrap()
        .current;

    for action_item in block_actions.iter().take(MAX_BLOCK_ACTIONS) {
        let get_pos = |content: &Option<
            jolyne::valentine::PlayerAuthInputPacketBlockActionItemContent,
        >|
         -> Option<(i32, i32, i32)> {
            content.as_ref().map(|c| {
                let pos = match c {
                    jolyne::valentine::PlayerAuthInputPacketBlockActionItemContent::PredictBreak(b) => &b.position,
                    jolyne::valentine::PlayerAuthInputPacketBlockActionItemContent::StartBreak(b) => &b.position,
                    jolyne::valentine::PlayerAuthInputPacketBlockActionItemContent::ContinueBreak(b) => &b.position,
                    jolyne::valentine::PlayerAuthInputPacketBlockActionItemContent::AbortBreak(b) => &b.position,
                    jolyne::valentine::PlayerAuthInputPacketBlockActionItemContent::CrackBreak(b) => &b.position,
                };
                (pos.x, pos.y, pos.z)
            })
        };

        trace!(?action_item.action, "Block action received");

        match action_item.action {
            Action::PredictBreak | Action::CreativePlayerDestroyBlock => {
                if let Some((x, y, z)) = get_pos(&action_item.content) {
                    trace!(pos = ?(x, y, z), "Creative/Predict block break");
                    break_block(world, player_entity, x, y, z);
                }
            }
            Action::StartBreak => {
                let pos = get_pos(&action_item.content);
                if let Some((x, y, z)) = pos {
                    let is_creative = world
                        .get::<crate::entity::components::GameMode>(player_entity)
                        .map(|gm| gm.instant_break())
                        .unwrap_or(false);

                    let break_time_ticks = if is_creative {
                        0
                    } else {
                        get_block_break_time(world, x, y, z)
                    };

                    trace!(pos = ?(x, y, z), is_creative, break_time_ticks, "StartBreak");

                    if let Some(mut event_buffer) = world.get_resource_mut::<EventBuffer>() {
                        event_buffer.push(ServerEvent::PlayerStartBreak {
                            entity: player_entity,
                            position: (x, y, z),
                            face: 0,
                        });
                    }

                    if let Some(mut breaking) = world.get_mut::<BreakingState>(player_entity) {
                        breaking.start(x, y, z, current_tick, break_time_ticks);
                    }

                    if !is_creative {
                        broadcast_block_crack_start(world, x, y, z, break_time_ticks);
                    }
                }
            }
            Action::CrackBreak | Action::ContinueBreak => {
                if let Some((x, y, z)) = get_pos(&action_item.content) {
                    let needs_start = world
                        .get::<BreakingState>(player_entity)
                        .map(|b| b.position.is_none())
                        .unwrap_or(true);

                    if needs_start {
                        let is_creative = world
                            .get::<crate::entity::components::GameMode>(player_entity)
                            .map(|gm| gm.instant_break())
                            .unwrap_or(false);

                        let break_time_ticks = if is_creative {
                            0
                        } else {
                            get_block_break_time(world, x, y, z)
                        };

                        trace!(pos = ?(x, y, z), is_creative, break_time_ticks, "CrackBreak: starting break");

                        if let Some(mut breaking) = world.get_mut::<BreakingState>(player_entity) {
                            breaking.start(x, y, z, current_tick, break_time_ticks);
                        }
                    }
                }
            }
            Action::StopBreak => {
                trace!("StopBreak received");
                let instant_break = world
                    .get::<crate::entity::components::GameMode>(player_entity)
                    .map(|gm| gm.instant_break())
                    .unwrap_or(false);
                let break_result = world
                    .get::<BreakingState>(player_entity)
                    .and_then(|breaking| {
                        if let Some((x, y, z)) = breaking.position {
                            let elapsed = current_tick.saturating_sub(breaking.start_tick);
                            trace!(pos = ?(x, y, z), elapsed, expected = breaking.expected_ticks, "StopBreak");
                            Some((x, y, z, instant_break || breaking.validate_break(current_tick)))
                        } else {
                            None
                        }
                    });

                if let Some((x, y, z, valid_break)) = break_result {
                    if valid_break {
                        break_block(world, player_entity, x, y, z);
                    } else {
                        warn!(
                            player = ?player_entity,
                            pos = ?(x, y, z),
                            "Rejected early block break"
                        );
                    }

                    if let Some(mut breaking) = world.get_mut::<BreakingState>(player_entity) {
                        breaking.stop();
                    }

                    broadcast_block_crack_stop(world, x, y, z);
                }
            }
            Action::AbortBreak => {
                if let Some((x, y, z)) = get_pos(&action_item.content) {
                    debug!(pos = ?(x, y, z), "AbortBreak");

                    if let Some(mut breaking) = world.get_mut::<BreakingState>(player_entity) {
                        breaking.stop();
                    }

                    broadcast_block_crack_stop(world, x, y, z);
                }
            }
            _ => {}
        }
    }
}

/// Broadcast block crack start animation to chunk viewers.
fn broadcast_block_crack_start(world: &World, x: i32, y: i32, z: i32, break_time_ticks: u32) {
    let (cx, cz) = world_to_chunk_coords(x, z);

    let Some(chunk_manager) = world.get_resource::<ChunkManager>() else {
        return;
    };
    let Some(chunk_entity) = chunk_manager.get_by_coords(cx, cz) else {
        return;
    };

    let break_data = 65535u32.checked_div(break_time_ticks).unwrap_or(65535) as i32;

    let packet = LevelEventPacket {
        event: LevelEventPacketEvent::BlockStartBreak,
        position: Vec3F {
            x: x as f32,
            y: y as f32,
            z: z as f32,
        },
        data: break_data,
    };

    if let Some(chunk_viewers) = world.get::<ChunkViewers>(chunk_entity) {
        for viewer_entity in chunk_viewers.iter() {
            if let Some(session) = world.get::<PlayerSession>(viewer_entity) {
                let _ = session.send(McpePacket::from(packet.clone()));
            }
        }
    }
}

/// Broadcast block crack stop animation to chunk viewers.
fn broadcast_block_crack_stop(world: &World, x: i32, y: i32, z: i32) {
    let (cx, cz) = world_to_chunk_coords(x, z);

    let Some(chunk_manager) = world.get_resource::<ChunkManager>() else {
        return;
    };
    let Some(chunk_entity) = chunk_manager.get_by_coords(cx, cz) else {
        return;
    };

    let packet = LevelEventPacket {
        event: LevelEventPacketEvent::BlockStopBreak,
        position: Vec3F {
            x: x as f32,
            y: y as f32,
            z: z as f32,
        },
        data: 0,
    };

    if let Some(chunk_viewers) = world.get::<ChunkViewers>(chunk_entity) {
        for viewer_entity in chunk_viewers.iter() {
            if let Some(session) = world.get::<PlayerSession>(viewer_entity) {
                let _ = session.send(McpePacket::from(packet.clone()));
            }
        }
    }
}

/// Break a block at world coordinates: set to air and broadcast to viewers.
fn break_block(world: &mut World, breaking_player: Entity, x: i32, y: i32, z: i32) {
    debug!(pos = ?(x, y, z), "break_block called");

    let (cx, cz) = world_to_chunk_coords(x, z);
    let (local_x, local_y, local_z) = world_to_local_coords(x, y, z);

    let chunk_entity = {
        let Some(chunk_manager) = world.get_resource::<ChunkManager>() else {
            return;
        };
        chunk_manager.get_by_coords(cx, cz)
    };

    let Some(chunk_entity) = chunk_entity else {
        debug!(chunk = ?(cx, cz), "break_block: chunk not found");
        return;
    };

    let original_block_id = world
        .get::<crate::world::ecs::ChunkData>(chunk_entity)
        .map(|cd| cd.inner.get_block(local_x, local_y, local_z))
        .unwrap_or(0);

    // Emit BlockBreak event
    let player_id = world
        .get::<PlayerUuid>(breaking_player)
        .map(|u| u.0.to_string())
        .unwrap_or_default();

    if let Some(mut event_buffer) = world.get_resource_mut::<EventBuffer>() {
        event_buffer.push(ServerEvent::BlockBreak {
            entity: breaking_player,
            player_id,
            position: (x, y, z),
            block_id: original_block_id,
        });
    }

    // Set block to air
    if let Some(mut chunk_data) = world.get_mut::<crate::world::ecs::ChunkData>(chunk_entity) {
        chunk_data
            .inner
            .set_block(local_x, local_y, local_z, *blocks::AIR);
    } else {
        return;
    }

    if let Some(mut state_flags) = world.get_mut::<crate::world::ecs::ChunkStateFlags>(chunk_entity)
    {
        state_flags.mark_needs_rebroadcast();
    }

    // Trigger observer and write message
    world.trigger(BlockChanged {
        chunk_entity,
        block_pos: IVec3::new(x, y, z),
        old_block: original_block_id,
        new_block: *blocks::AIR,
    });

    world.write_message(BlockBroadcastEvent {
        chunk_entity,
        block_pos: IVec3::new(x, y, z),
        new_block: *blocks::AIR,
    });

    // Spawn item drop in survival
    if original_block_id != *blocks::AIR {
        let is_survival = world
            .get::<crate::entity::components::GameMode>(breaking_player)
            .map(|gm| !gm.instant_break())
            .unwrap_or(true);

        if is_survival {
            // Look up the block by runtime ID to get its string_id,
            // then find the matching item to get its network_id
            let item_network_id = world
                .get_resource::<super::types::BlockRegistryResource>()
                .and_then(|blocks| {
                    blocks
                        .0
                        .get_by_runtime_id(original_block_id)
                        .map(|b| b.string_id.clone())
                })
                .and_then(|block_string_id| {
                    world
                        .get_resource::<super::types::ItemRegistryResource>()
                        .and_then(|items| {
                            items
                                .0
                                .get_by_name(&block_string_id)
                                .map(|item| item.network_id)
                        })
                })
                .unwrap_or(0);

            if item_network_id != 0 {
                let item_entity_id = {
                    let mut counter = world
                        .get_resource_mut::<super::types::ItemEntityIdCounter>()
                        .unwrap();
                    let id = counter.0;
                    counter.0 += 1;
                    id
                };

                use jolyne::valentine::AddItemEntityPacket;
                use jolyne::valentine::types::{
                    Item, ItemContent, ItemContentExtra, ItemExtraDataWithoutBlockingTick,
                };

                let item_packet = AddItemEntityPacket {
                    entity_id_self: item_entity_id,
                    runtime_entity_id: item_entity_id,
                    item: Item {
                        network_id: item_network_id,
                        content: Some(Box::new(ItemContent {
                            count: 1,
                            metadata: 0,
                            has_stack_id: 0,
                            stack_id: None,
                            block_runtime_id: original_block_id as i32,
                            extra: ItemContentExtra::Default(
                                ItemExtraDataWithoutBlockingTick::default(),
                            ),
                        })),
                    },
                    position: Vec3F {
                        x: x as f32 + 0.5,
                        y: y as f32 + 0.25,
                        z: z as f32 + 0.5,
                    },
                    velocity: Vec3F {
                        x: 0.0,
                        y: 0.1,
                        z: 0.0,
                    },
                    metadata: vec![],
                    is_from_fishing: false,
                };

                if let Some(session) = world.get::<PlayerSession>(breaking_player) {
                    let _ = session.send(McpePacket::from(item_packet.clone()));
                    info!(pos = ?(x, y, z), item_network_id, entity_id = item_entity_id, "Spawned item drop");
                }

                if let Some(chunk_viewers) = world.get::<ChunkViewers>(chunk_entity) {
                    for viewer in chunk_viewers.iter() {
                        if viewer != breaking_player
                            && let Some(viewer_session) = world.get::<PlayerSession>(viewer)
                        {
                            let _ = viewer_session.send(McpePacket::from(item_packet.clone()));
                        }
                    }
                }
            }
        }
    }

    // Broadcast update to viewers
    broadcast_block_break(
        world,
        breaking_player,
        chunk_entity,
        x,
        y,
        z,
        original_block_id,
    );
}

/// Broadcast block break effects (UpdateBlock, particles, sound) to viewers.
fn broadcast_block_break(
    world: &World,
    breaking_player: Entity,
    chunk_entity: Entity,
    x: i32,
    y: i32,
    z: i32,
    original_block_id: u32,
) {
    use jolyne::valentine::types::{SoundType, UpdateBlockFlags};
    use jolyne::valentine::{LevelSoundEventPacket, UpdateBlockPacket};

    let update_packet = UpdateBlockPacket {
        position: BlockCoordinates { x, y, z },
        block_runtime_id: *blocks::AIR as i32,
        flags: UpdateBlockFlags::NEIGHBORS | UpdateBlockFlags::NETWORK,
        layer: 0,
    };

    let particle_packet = LevelEventPacket {
        event: LevelEventPacketEvent::ParticleDestroy,
        position: Vec3F {
            x: x as f32 + 0.5,
            y: y as f32 + 0.5,
            z: z as f32 + 0.5,
        },
        data: original_block_id as i32,
    };

    let sound_packet = LevelSoundEventPacket {
        sound_id: SoundType::BreakBlock,
        position: Vec3F {
            x: x as f32 + 0.5,
            y: y as f32 + 0.5,
            z: z as f32 + 0.5,
        },
        extra_data: original_block_id as i32,
        entity_type: String::new(),
        is_baby_mob: false,
        is_global: false,
        entity_unique_id: 0,
    };

    if let Some(chunk_viewers) = world.get::<ChunkViewers>(chunk_entity) {
        let mut sent_to = std::collections::HashSet::new();

        for viewer_entity in chunk_viewers.iter() {
            if let Some(session) = world.get::<PlayerSession>(viewer_entity) {
                let _ = session.send(McpePacket::from(update_packet.clone()));
                let _ = session.send(McpePacket::from(particle_packet.clone()));
                let _ = session.send(McpePacket::from(sound_packet.clone()));
                sent_to.insert(viewer_entity);
            }
        }

        if !sent_to.contains(&breaking_player)
            && let Some(session) = world.get::<PlayerSession>(breaking_player)
        {
            let _ = session.send(McpePacket::from(update_packet));
            let _ = session.send(McpePacket::from(particle_packet));
            let _ = session.send(McpePacket::from(sound_packet));
        }
    } else if let Some(session) = world.get::<PlayerSession>(breaking_player) {
        let _ = session.send(McpePacket::from(update_packet));
        let _ = session.send(McpePacket::from(particle_packet));
        let _ = session.send(McpePacket::from(sound_packet));
    }
}

/// Get break time in ticks for the block at given world coordinates.
fn get_block_break_time(world: &World, x: i32, y: i32, z: i32) -> u32 {
    let (cx, cz) = world_to_chunk_coords(x, z);
    let (local_x, local_y, local_z) = world_to_local_coords(x, y, z);

    let block_runtime_id = {
        let Some(chunk_manager) = world.get_resource::<ChunkManager>() else {
            return 20;
        };
        let Some(chunk_entity) = chunk_manager.get_by_coords(cx, cz) else {
            return 20;
        };
        let Some(chunk_data) = world.get::<crate::world::ecs::ChunkData>(chunk_entity) else {
            return 20;
        };
        chunk_data.inner.get_block(local_x, local_y, local_z)
    };

    if let Some(blocks) = world.get_resource::<super::types::BlockRegistryResource>()
        && let Some(block) = blocks.0.get_by_runtime_id(block_runtime_id)
    {
        if block.hardness < 0.0 {
            return u32::MAX;
        }
        if block.hardness <= 0.0 {
            return 1;
        }
        return (block.hardness * 5.0 * 20.0).ceil() as u32;
    }

    20
}

/// Handle block click from InventoryTransaction ItemUse::ClickBlock.
fn handle_block_click(
    world: &mut World,
    entity: Entity,
    data: &jolyne::valentine::types::TransactionUseItem,
) {
    // Emit PlayerInteractBlock event
    if let Some(mut event_buffer) = world.get_resource_mut::<EventBuffer>() {
        event_buffer.push(ServerEvent::PlayerInteractBlock {
            entity,
            position: (
                data.block_position.x,
                data.block_position.y,
                data.block_position.z,
            ),
            face: data.face as u8,
        });
    }

    let network_id = data.held_item.network_id;
    if network_id == 0 {
        return;
    }

    // Map item -> block
    let block_runtime_id = {
        let Some(items) = world.get_resource::<super::types::ItemRegistryResource>() else {
            return;
        };
        let Some(block_reg) = world.get_resource::<super::types::BlockRegistryResource>() else {
            return;
        };
        if let Some(item_entry) = items.0.get_by_network_id(network_id) {
            if let Some(block_entry) = block_reg.0.get_by_name(&item_entry.string_id) {
                debug!(
                    network_id,
                    string_id = %item_entry.string_id,
                    default_state = block_entry.default_state_id,
                    "Block click: mapped item to block"
                );
                block_entry.default_state_id
            } else {
                debug!(network_id, string_id = %item_entry.string_id, "Not a block");
                return;
            }
        } else {
            debug!(network_id, "Unknown item");
            return;
        }
    };

    // Calculate placement position
    let mut x = data.block_position.x;
    let mut y = data.block_position.y;
    let mut z = data.block_position.z;

    match data.face {
        0 => y -= 1,
        1 => y += 1,
        2 => z -= 1,
        3 => z += 1,
        4 => x -= 1,
        5 => x += 1,
        _ => return,
    }

    // Emit BlockPlace event
    let player_id = world
        .get::<PlayerUuid>(entity)
        .map(|u| u.0.to_string())
        .unwrap_or_default();

    if let Some(mut event_buffer) = world.get_resource_mut::<EventBuffer>() {
        event_buffer.push(ServerEvent::BlockPlace {
            entity,
            player_id,
            position: (x, y, z),
            block_id: block_runtime_id,
        });
    }

    place_block(world, x, y, z, block_runtime_id);
}

/// Place a block at world coordinates: update chunk and broadcast.
fn place_block(world: &mut World, x: i32, y: i32, z: i32, block_runtime_id: u32) {
    let (cx, cz) = world_to_chunk_coords(x, z);
    let (local_x, local_y, local_z) = world_to_local_coords(x, y, z);

    let chunk_entity = {
        let Some(chunk_manager) = world.get_resource::<ChunkManager>() else {
            return;
        };
        chunk_manager.get_by_coords(cx, cz)
    };

    let Some(chunk_entity) = chunk_entity else {
        debug!(chunk = ?(cx, cz), "place_block: chunk not found");
        return;
    };

    let old_block_id = world
        .get::<crate::world::ecs::ChunkData>(chunk_entity)
        .map(|cd| cd.inner.get_block(local_x, local_y, local_z))
        .unwrap_or(0);

    // Update chunk data
    if let Some(mut chunk_data) = world.get_mut::<crate::world::ecs::ChunkData>(chunk_entity) {
        chunk_data
            .inner
            .set_block(local_x, local_y, local_z, block_runtime_id);
    } else {
        return;
    }

    if let Some(mut state_flags) = world.get_mut::<crate::world::ecs::ChunkStateFlags>(chunk_entity)
    {
        state_flags.mark_needs_rebroadcast();
    }

    world.trigger(BlockChanged {
        chunk_entity,
        block_pos: IVec3::new(x, y, z),
        old_block: old_block_id,
        new_block: block_runtime_id,
    });

    world.write_message(BlockBroadcastEvent {
        chunk_entity,
        block_pos: IVec3::new(x, y, z),
        new_block: block_runtime_id,
    });

    // Broadcast to viewers
    if let Some(chunk_viewers) = world.get::<ChunkViewers>(chunk_entity) {
        use jolyne::valentine::types::{SoundType, UpdateBlockFlags};
        use jolyne::valentine::{LevelSoundEventPacket, UpdateBlockPacket};

        let update_packet = UpdateBlockPacket {
            position: BlockCoordinates { x, y, z },
            block_runtime_id: block_runtime_id as i32,
            flags: UpdateBlockFlags::NEIGHBORS | UpdateBlockFlags::NETWORK,
            layer: 0,
        };

        let sound_packet = LevelSoundEventPacket {
            sound_id: SoundType::Place,
            position: Vec3F {
                x: x as f32 + 0.5,
                y: y as f32 + 0.5,
                z: z as f32 + 0.5,
            },
            extra_data: block_runtime_id as i32,
            entity_type: String::new(),
            is_baby_mob: false,
            is_global: false,
            entity_unique_id: 0,
        };

        for viewer_entity in chunk_viewers.iter() {
            if let Some(session) = world.get::<PlayerSession>(viewer_entity) {
                let _ = session.send(McpePacket::from(update_packet.clone()));
                let _ = session.send(McpePacket::from(sound_packet.clone()));
            }
        }
    }
}
