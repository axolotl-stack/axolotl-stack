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
use jolyne::valentine::types::{BlockPos, EnumsPlayerActionType, Vec3};
use jolyne::valentine::{LevelEventPacket, McpePacket};

// Protocol 2168 carries these event IDs as canonical i32 values. The values
// are the LevelEvent constants used by Dragonfly/gophertunnel.
const LEVEL_EVENT_BLOCK_START_BREAK: i32 = 3600;
const LEVEL_EVENT_BLOCK_STOP_BREAK: i32 = 3601;
const LEVEL_EVENT_PARTICLES_DESTROY_BLOCK: i32 = 2001;
const BLOCK_UPDATE_NEIGHBOURS: u32 = 1;
const BLOCK_UPDATE_NETWORK: u32 = 1 << 1;

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
    block_actions: &[jolyne::valentine::PlayerBlockActionData],
) {
    let current_tick = world
        .get_resource::<crate::ecs::resources::TickCounter>()
        .unwrap()
        .current;

    for action_item in block_actions.iter().take(MAX_BLOCK_ACTIONS) {
        let (x, y, z) = (
            action_item.position.x,
            action_item.position.y,
            action_item.position.z,
        );

        trace!(action = ?action_item.player_action_type, pos = ?(x, y, z), "Block action received");
        if !can_interact(world, player_entity, (x, y, z))
            && !matches!(
                action_item.player_action_type,
                EnumsPlayerActionType::Abortdestroyblock | EnumsPlayerActionType::Stopdestroyblock
            )
        {
            correct_block(world, player_entity, (x, y, z));
            continue;
        }

        match action_item.player_action_type {
            EnumsPlayerActionType::Predictdestroyblock
            | EnumsPlayerActionType::Creativedestroyblock => {
                trace!(pos = ?(x, y, z), "Creative/Predict block break");
                break_block(world, player_entity, x, y, z);
            }
            EnumsPlayerActionType::Startdestroyblock => {
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
            EnumsPlayerActionType::Crackblock | EnumsPlayerActionType::Continuedestroyblock => {
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
            EnumsPlayerActionType::Stopdestroyblock => {
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
                        correct_block(world, player_entity, (x, y, z));
                    }

                    if let Some(mut breaking) = world.get_mut::<BreakingState>(player_entity) {
                        breaking.stop();
                    }

                    broadcast_block_crack_stop(world, x, y, z);
                }
            }
            EnumsPlayerActionType::Abortdestroyblock => {
                debug!(pos = ?(x, y, z), "AbortBreak");

                if let Some(mut breaking) = world.get_mut::<BreakingState>(player_entity) {
                    breaking.stop();
                }

                broadcast_block_crack_stop(world, x, y, z);
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
        event_id: LEVEL_EVENT_BLOCK_START_BREAK,
        position: Vec3 {
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
        event_id: LEVEL_EVENT_BLOCK_STOP_BREAK,
        position: Vec3 {
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

fn block_at(world: &World, position: (i32, i32, i32)) -> Option<u32> {
    let (x, y, z) = position;
    if !(crate::world::chunk::MIN_Y
        ..crate::world::chunk::MIN_Y + i32::from(crate::world::chunk::SUBCHUNK_COUNT) * 16)
        .contains(&y)
    {
        return None;
    }
    let (cx, cz) = world_to_chunk_coords(x, z);
    let chunk = world
        .get_resource::<ChunkManager>()?
        .get_by_coords(cx, cz)?;
    let (x, y, z) = world_to_local_coords(x, y, z);
    Some(
        world
            .get::<crate::world::ecs::ChunkData>(chunk)?
            .inner
            .get_block(x, y, z),
    )
}

/// Reach is measured from server-tracked position to the target block's box.
/// Missing/non-finite player state and non-building modes fail closed.
fn can_interact(world: &World, entity: Entity, position: (i32, i32, i32)) -> bool {
    use crate::entity::components::{GameMode, Position};
    let Some(mode) = world.get::<GameMode>(entity) else {
        return false;
    };
    let Some(origin) = world.get::<Position>(entity).map(|position| position.0) else {
        return false;
    };
    if !mode.can_break_blocks() || !origin.is_finite() || block_at(world, position).is_none() {
        return false;
    }
    let min = glam::DVec3::new(
        f64::from(position.0),
        f64::from(position.1),
        f64::from(position.2),
    );
    let nearest = origin.clamp(min, min + glam::DVec3::ONE);
    let reach = if *mode == GameMode::Creative {
        6.0
    } else {
        5.0
    };
    origin.distance_squared(nearest) <= reach * reach
}

/// Resolve placeable content exclusively from the server's selected stack.
fn held_block(world: &World, entity: Entity) -> Option<(u32, &crate::item::ItemStack, i32)> {
    use crate::entity::components::{HeldSlot, MainInventory};
    let slot = world.get::<HeldSlot>(entity)?.0;
    if slot > 8 {
        return None;
    }
    let stack = world
        .get::<MainInventory>(entity)?
        .0
        .item(usize::from(slot))?;
    if stack.is_empty() {
        return None;
    }
    let item = world
        .get_resource::<super::types::ItemRegistryResource>()?
        .0
        .get_by_name(&stack.item_id)?;
    if stack.count > item.stack_size {
        return None;
    }
    let block = world
        .get_resource::<super::types::BlockRegistryResource>()?
        .0
        .get_by_name(&stack.item_id)?;
    Some((block.default_state_id, stack, item.network_id))
}

fn correct_block(world: &World, entity: Entity, position: (i32, i32, i32)) {
    let Some(block_runtime_id) = block_at(world, position) else {
        return;
    };
    if let Some(session) = world.get::<PlayerSession>(entity) {
        let _ = session.send(McpePacket::from(jolyne::valentine::UpdateBlockPacket {
            block_position: BlockPos {
                x: position.0,
                y: position.1,
                z: position.2,
            },
            block_runtime_id,
            flags: BLOCK_UPDATE_NEIGHBOURS | BLOCK_UPDATE_NETWORK,
            layer: 0,
        }));
    }
}

/// Called only after the plugin manager has accepted this queued intent.
/// Revalidation closes races between packet processing and plugin dispatch.
pub(crate) fn commit_block_event(world: &mut World, event: &ServerEvent) {
    match event {
        ServerEvent::BlockBreak {
            entity,
            position,
            block_id,
            ..
        } => {
            if can_interact(world, *entity, *position)
                && block_at(world, *position) == Some(*block_id)
            {
                apply_break_block(world, *entity, position.0, position.1, position.2);
            } else {
                reject_block_event(world, event);
            }
        }
        ServerEvent::BlockPlace {
            entity,
            position,
            block_id,
            ..
        } => {
            let valid = can_interact(world, *entity, *position)
                && block_at(world, *position) == Some(*blocks::AIR)
                && held_block(world, *entity).is_some_and(|(held, _, _)| held == *block_id);
            if !valid {
                reject_block_event(world, event);
                return;
            }
            place_block(world, position.0, position.1, position.2, *block_id);
            if world.get::<crate::entity::components::GameMode>(*entity)
                == Some(&crate::entity::components::GameMode::Survival)
            {
                let slot = usize::from(
                    world
                        .get::<crate::entity::components::HeldSlot>(*entity)
                        .unwrap()
                        .0,
                );
                let mut inventory = world
                    .get_mut::<crate::entity::components::MainInventory>(*entity)
                    .unwrap();
                let item = inventory.0.item_mut(slot).unwrap();
                *item = item.grow(-1);
            }
            super::inventory::resync_inventory(world, *entity);
            correct_block(world, *entity, *position);
        }
        _ => {}
    }
}

pub(crate) fn reject_block_event(world: &World, event: &ServerEvent) {
    match event {
        ServerEvent::BlockBreak {
            entity, position, ..
        }
        | ServerEvent::BlockPlace {
            entity, position, ..
        } => {
            correct_block(world, *entity, *position);
            super::inventory::resync_inventory(world, *entity);
        }
        _ => {}
    }
}

/// Validate and queue a break intent. Plugin dispatch owns the commit boundary.
fn break_block(world: &mut World, player: Entity, x: i32, y: i32, z: i32) {
    let position = (x, y, z);
    let creative = world
        .get::<crate::entity::components::GameMode>(player)
        .is_some_and(|mode| mode.instant_break());
    let tick = world
        .get_resource::<crate::ecs::resources::TickCounter>()
        .map_or(0, |tick| tick.current);
    let mined = world.get::<BreakingState>(player).is_some_and(|state| {
        state.position == Some(position)
            && state.expected_ticks != u32::MAX
            && tick.saturating_sub(state.start_tick) >= u64::from(state.expected_ticks) * 9 / 10
    });
    if !can_interact(world, player, position) || (!creative && !mined) {
        correct_block(world, player, position);
        return;
    }
    let Some(block_id) = block_at(world, position) else {
        return;
    };
    if block_id == *blocks::AIR || (!creative && get_block_break_time(world, x, y, z) == u32::MAX) {
        correct_block(world, player, position);
        return;
    }
    let player_id = world
        .get::<PlayerUuid>(player)
        .map(|id| id.0.to_string())
        .unwrap_or_default();
    if let Some(mut events) = world.get_resource_mut::<EventBuffer>() {
        events.push(ServerEvent::BlockBreak {
            entity: player,
            player_id,
            position,
            block_id,
        });
    }
}

/// Apply an already approved break; never called before cancellable dispatch.
fn apply_break_block(world: &mut World, breaking_player: Entity, x: i32, y: i32, z: i32) {
    debug!(pos = ?(x, y, z), "Committing block break");

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
            let (item_id, item_network_id) = world
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
                                .map(|item| (item.string_id.clone(), item.network_id))
                        })
                })
                .unwrap_or_default();

            if item_network_id != 0 {
                let item_entity_id = {
                    let mut counter = world
                        .get_resource_mut::<super::types::ItemEntityIdCounter>()
                        .unwrap();
                    let id = counter.0;
                    counter.0 += 1;
                    id
                };

                use jolyne::valentine::AddItemActorPacket;
                use jolyne::valentine::types::{
                    ActorRuntimeId, ActorUniqueId,
                    CerealizerNetworkItemStackDescriptorSerializedData,
                    SynchedActorDataCopyableDataList, Vec3,
                };

                let item_packet = match i16::try_from(item_network_id) {
                    Ok(network_id) => Some(AddItemActorPacket {
                        target_actor_id: ActorUniqueId {
                            actor_unique_id: item_entity_id,
                        },
                        target_runtime_id: ActorRuntimeId {
                            actor_runtime_id: item_entity_id as u64,
                        },
                        item: CerealizerNetworkItemStackDescriptorSerializedData {
                            id: network_id,
                            stacksize: 1,
                            auxvalue: 0,
                            net_id_variant: None,
                            block_runtime_id: original_block_id,
                            user_data_buffer: crate::item::user_data::network_user_data(
                                &item_id, None,
                            ),
                        },
                        position: Vec3 {
                            x: x as f32 + 0.5,
                            y: y as f32 + 0.25,
                            z: z as f32 + 0.5,
                        },
                        velocity: Vec3 {
                            x: 0.0,
                            y: 0.1,
                            z: 0.0,
                        },
                        entity_data: SynchedActorDataCopyableDataList::default(),
                        is_from_fishing: false,
                    }),
                    Err(_) => {
                        warn!(
                            item_network_id,
                            "Skipping item drop outside protocol ID range"
                        );
                        None
                    }
                };

                if let Some(item_packet) = item_packet {
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
    use jolyne::valentine::{LevelSoundEventPacket, UpdateBlockPacket};

    let update_packet = UpdateBlockPacket {
        block_position: BlockPos { x, y, z },
        block_runtime_id: *blocks::AIR,
        flags: BLOCK_UPDATE_NEIGHBOURS | BLOCK_UPDATE_NETWORK,
        layer: 0,
    };

    let particle_packet = LevelEventPacket {
        event_id: LEVEL_EVENT_PARTICLES_DESTROY_BLOCK,
        position: Vec3 {
            x: x as f32 + 0.5,
            y: y as f32 + 0.5,
            z: z as f32 + 0.5,
        },
        data: original_block_id as i32,
    };

    let sound_packet = LevelSoundEventPacket {
        sound_event: "break.block".into(),
        position: Vec3 {
            x: x as f32 + 0.5,
            y: y as f32 + 0.5,
            z: z as f32 + 0.5,
        },
        data: original_block_id as i32,
        actor_identifier: "".into(),
        is_baby: false,
        is_global: false,
        actor_unique_id: 0,
        fire_at_position: None,
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
    data: &jolyne::valentine::types::ItemUseInventoryTransaction,
) {
    let clicked = (data.position.x, data.position.y, data.position.z);
    let offset = match data.face {
        0 => (0, -1, 0),
        1 => (0, 1, 0),
        2 => (0, 0, -1),
        3 => (0, 0, 1),
        4 => (-1, 0, 0),
        5 => (1, 0, 0),
        _ => {
            correct_block(world, entity, clicked);
            return;
        }
    };
    let Some(position) = clicked
        .0
        .checked_add(offset.0)
        .zip(clicked.1.checked_add(offset.1))
        .zip(clicked.2.checked_add(offset.2))
        .map(|((x, y), z)| (x, y, z))
    else {
        return;
    };
    if !can_interact(world, entity, clicked) {
        correct_block(world, entity, clicked);
        correct_block(world, entity, position);
        super::inventory::resync_inventory(world, entity);
        return;
    }
    // A valid placement follows this interaction immediately in EventBuffer.
    // PluginManager carries interaction cancellation to the paired placement.
    if let Some(mut events) = world.get_resource_mut::<EventBuffer>() {
        events.push(ServerEvent::PlayerInteractBlock {
            entity,
            position: clicked,
            face: data.face,
        });
    }
    let held = held_block(world, entity);
    let matches_held = held.as_ref().is_some_and(|(_, stack, network_id)| {
        data.hand == jolyne::valentine::EnumsHandSlot::Mainhand
            && world
                .get::<crate::entity::components::HeldSlot>(entity)
                .is_some_and(|slot| data.slot == i32::from(slot.0))
            && i32::from(data.item.id) == *network_id
            && data.item.stacksize == u16::from(stack.count)
            && u32::try_from(stack.damage).ok() == Some(data.item.auxvalue)
            && crate::item::user_data::stored_user_data(&stack.item_id, &data.item.user_data_buffer)
                == stack.nbt
            && u32::try_from(stack.runtime_id).ok() == Some(data.item.block_runtime_id)
    });
    if !matches_held
        || !can_interact(world, entity, clicked)
        || !can_interact(world, entity, position)
        || block_at(world, clicked).is_none_or(|block| block == *blocks::AIR)
        || block_at(world, position) != Some(*blocks::AIR)
    {
        correct_block(world, entity, clicked);
        correct_block(world, entity, position);
        super::inventory::resync_inventory(world, entity);
        return;
    }
    let block_id = held.unwrap().0;
    let player_id = world
        .get::<PlayerUuid>(entity)
        .map(|id| id.0.to_string())
        .unwrap_or_default();
    if let Some(mut events) = world.get_resource_mut::<EventBuffer>() {
        events.push(ServerEvent::BlockPlace {
            entity,
            player_id,
            position,
            block_id,
        });
    }
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
        use jolyne::valentine::{LevelSoundEventPacket, UpdateBlockPacket};

        let update_packet = UpdateBlockPacket {
            block_position: BlockPos { x, y, z },
            block_runtime_id,
            flags: BLOCK_UPDATE_NEIGHBOURS | BLOCK_UPDATE_NETWORK,
            layer: 0,
        };

        let sound_packet = LevelSoundEventPacket {
            sound_event: "place".into(),
            position: Vec3 {
                x: x as f32 + 0.5,
                y: y as f32 + 0.5,
                z: z as f32 + 0.5,
            },
            data: block_runtime_id as i32,
            actor_identifier: "".into(),
            is_baby: false,
            is_global: false,
            actor_unique_id: 0,
            fire_at_position: None,
        };

        for viewer_entity in chunk_viewers.iter() {
            if let Some(session) = world.get::<PlayerSession>(viewer_entity) {
                let _ = session.send(McpePacket::from(update_packet.clone()));
                let _ = session.send(McpePacket::from(sound_packet.clone()));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::components::{GameMode, HeldSlot, MainInventory, Position};
    use crate::world::ecs::{ChunkData, ChunkPosition};
    use glam::DVec3;

    fn block_world(mode: GameMode, position: DVec3) -> (World, Entity, Entity) {
        let mut world = World::new();
        world.insert_resource(ChunkManager::new(crate::world::WorldConfig::default()));
        world.insert_resource(EventBuffer::default());
        world.insert_resource(Messages::<BlockBroadcastEvent>::default());
        let mut chunk = crate::world::chunk::Chunk::new(0, 0);
        chunk.set_block(1, 64, 1, *blocks::STONE);
        let chunk = world.spawn(ChunkData::new(chunk)).id();
        world
            .resource_mut::<ChunkManager>()
            .insert(ChunkPosition::new(0, 0), chunk);
        let player = world
            .spawn((
                mode,
                Position(position),
                MainInventory::default(),
                HeldSlot::default(),
            ))
            .id();
        (world, player, chunk)
    }

    #[test]
    fn distant_break_leaves_block_unchanged() {
        let (mut world, player, chunk) =
            block_world(GameMode::Creative, DVec3::new(100.0, 64.0, 100.0));
        break_block(&mut world, player, 1, 64, 1);
        assert_eq!(
            world
                .get::<ChunkData>(chunk)
                .unwrap()
                .inner
                .get_block(1, 64, 1),
            *blocks::STONE
        );
    }

    #[test]
    fn adventure_break_leaves_block_unchanged() {
        let (mut world, player, chunk) =
            block_world(GameMode::Adventure, DVec3::new(1.0, 64.0, 1.0));
        break_block(&mut world, player, 1, 64, 1);
        assert_eq!(
            world
                .get::<ChunkData>(chunk)
                .unwrap()
                .inner
                .get_block(1, 64, 1),
            *blocks::STONE
        );
    }

    #[test]
    fn creative_break_is_only_an_intent_until_plugins_allow_it() {
        let (mut world, player, chunk) =
            block_world(GameMode::Creative, DVec3::new(1.0, 64.0, 1.0));
        break_block(&mut world, player, 1, 64, 1);
        assert_eq!(
            world
                .get::<ChunkData>(chunk)
                .unwrap()
                .inner
                .get_block(1, 64, 1),
            *blocks::STONE
        );
        assert!(matches!(
            world.resource_mut::<EventBuffer>().drain().as_slice(),
            [ServerEvent::BlockBreak { .. }]
        ));
    }

    fn placement(world: &mut World) -> jolyne::valentine::ItemUseInventoryTransaction {
        use crate::registry::{BlockRegistry, ItemRegistry};
        use std::sync::Arc;
        let mut items = ItemRegistry::new();
        items.load_vanilla();
        let id = items.get_by_name("minecraft:stone").unwrap().network_id;
        world.insert_resource(super::super::types::ItemRegistryResource(Arc::new(items)));
        world.insert_resource(super::super::types::BlockRegistryResource(Arc::new(
            BlockRegistry::vanilla().clone(),
        )));
        jolyne::valentine::ItemUseInventoryTransaction {
            position: BlockPos { x: 1, y: 64, z: 1 },
            face: 1,
            slot: 0,
            item: jolyne::valentine::CerealizerNetworkItemStackDescriptorSerializedData {
                id: i16::try_from(id).unwrap(),
                stacksize: 2,
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[test]
    fn placement_without_owned_item_is_rejected() {
        let (mut world, player, chunk) =
            block_world(GameMode::Survival, DVec3::new(1.0, 64.0, 1.0));
        let data = placement(&mut world);
        handle_block_click(&mut world, player, &data);
        assert_eq!(
            world
                .get::<ChunkData>(chunk)
                .unwrap()
                .inner
                .get_block(1, 65, 1),
            *blocks::AIR
        );
        assert!(
            !world
                .resource_mut::<EventBuffer>()
                .drain()
                .iter()
                .any(|event| matches!(event, ServerEvent::BlockPlace { .. }))
        );
    }

    #[test]
    fn placement_waits_for_plugins_before_world_or_inventory_changes() {
        let (mut world, player, chunk) =
            block_world(GameMode::Survival, DVec3::new(1.0, 64.0, 1.0));
        let data = placement(&mut world);
        world
            .get_mut::<MainInventory>(player)
            .unwrap()
            .0
            .set_item(0, crate::item::ItemStack::new("minecraft:stone", 2))
            .unwrap();
        handle_block_click(&mut world, player, &data);
        assert_eq!(
            world
                .get::<ChunkData>(chunk)
                .unwrap()
                .inner
                .get_block(1, 65, 1),
            *blocks::AIR
        );
        assert_eq!(
            world
                .get::<MainInventory>(player)
                .unwrap()
                .0
                .item(0)
                .unwrap()
                .count,
            2
        );
        assert!(
            world
                .resource_mut::<EventBuffer>()
                .drain()
                .iter()
                .any(|event| matches!(event, ServerEvent::BlockPlace { .. }))
        );
    }

    #[test]
    fn allowed_survival_place_consumes_exactly_one_and_cannot_replay_over_block() {
        let (mut world, player, chunk) =
            block_world(GameMode::Survival, DVec3::new(1.0, 64.0, 1.0));
        let data = placement(&mut world);
        world
            .get_mut::<MainInventory>(player)
            .unwrap()
            .0
            .set_item(0, crate::item::ItemStack::new("minecraft:stone", 2))
            .unwrap();
        handle_block_click(&mut world, player, &data);
        let event = world.resource_mut::<EventBuffer>().drain().pop().unwrap();
        commit_block_event(&mut world, &event);
        assert_eq!(
            world
                .get::<ChunkData>(chunk)
                .unwrap()
                .inner
                .get_block(1, 65, 1),
            *blocks::STONE
        );
        assert_eq!(
            world
                .get::<MainInventory>(player)
                .unwrap()
                .0
                .item(0)
                .unwrap()
                .count,
            1
        );
        commit_block_event(&mut world, &event);
        assert_eq!(
            world
                .get::<MainInventory>(player)
                .unwrap()
                .0
                .item(0)
                .unwrap()
                .count,
            1
        );
    }

    #[test]
    fn deferred_place_revalidates_inventory_and_mode_before_commit() {
        for remove_item in [false, true] {
            let (mut world, player, chunk) =
                block_world(GameMode::Survival, DVec3::new(1.0, 64.0, 1.0));
            let data = placement(&mut world);
            world
                .get_mut::<MainInventory>(player)
                .unwrap()
                .0
                .set_item(0, crate::item::ItemStack::new("minecraft:stone", 2))
                .unwrap();
            handle_block_click(&mut world, player, &data);
            let event = world.resource_mut::<EventBuffer>().drain().pop().unwrap();
            if remove_item {
                world
                    .get_mut::<MainInventory>(player)
                    .unwrap()
                    .0
                    .set_item(0, crate::item::ItemStack::empty())
                    .unwrap();
            } else {
                *world.get_mut::<GameMode>(player).unwrap() = GameMode::Spectator;
            }
            commit_block_event(&mut world, &event);
            assert_eq!(
                world
                    .get::<ChunkData>(chunk)
                    .unwrap()
                    .inner
                    .get_block(1, 65, 1),
                *blocks::AIR
            );
        }
    }

    #[test]
    fn survival_prediction_cannot_skip_mining_duration() {
        let (mut world, player, chunk) =
            block_world(GameMode::Survival, DVec3::new(1.0, 64.0, 1.0));
        world.insert_resource(crate::ecs::resources::TickCounter { current: 2 });
        let mut state = BreakingState::default();
        state.start(1, 64, 1, 1, 20);
        world.entity_mut(player).insert(state);
        break_block(&mut world, player, 1, 64, 1);
        assert!(world.resource_mut::<EventBuffer>().drain().is_empty());
        world
            .resource_mut::<crate::ecs::resources::TickCounter>()
            .current = 20;
        break_block(&mut world, player, 1, 64, 1);
        let event = world.resource_mut::<EventBuffer>().drain().pop().unwrap();
        commit_block_event(&mut world, &event);
        assert_eq!(
            world
                .get::<ChunkData>(chunk)
                .unwrap()
                .inner
                .get_block(1, 64, 1),
            *blocks::AIR
        );
    }

    #[test]
    fn forged_placement_sends_authoritative_block_and_inventory_corrections() {
        use jolyne::valentine::McpePacketData;
        let (mut world, player, _) = block_world(GameMode::Survival, DVec3::new(1.0, 64.0, 1.0));
        let data = placement(&mut world);
        let (tx, mut rx) = tokio::sync::mpsc::channel(8);
        world
            .entity_mut(player)
            .insert(PlayerSession::new(1, "test".into(), None, None, tx));
        handle_block_click(&mut world, player, &data);
        let McpePacketData::UpdateBlockPacket(clicked) = rx.try_recv().unwrap().data else {
            panic!("clicked block correction")
        };
        assert_eq!(clicked.block_position, data.position);
        assert_eq!(clicked.block_runtime_id, *blocks::STONE);
        let McpePacketData::UpdateBlockPacket(placed) = rx.try_recv().unwrap().data else {
            panic!("predicted block correction")
        };
        assert_eq!(placed.block_position, BlockPos { x: 1, y: 65, z: 1 });
        assert_eq!(placed.block_runtime_id, *blocks::AIR);
        let McpePacketData::InventoryContentPacket(inventory) = rx.try_recv().unwrap().data else {
            panic!("inventory correction")
        };
        assert!(
            inventory
                .slots
                .iter()
                .all(|slot| slot.id == 0 && slot.stacksize == 0)
        );
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn plugin_given_block_echo_can_be_placed_and_consumed() {
        use crate::ecs::events::{ActionQueue, PluginAction};
        use crate::entity::components::{Rotation, RuntimeEntityId};
        let (mut world, player, chunk) =
            block_world(GameMode::Survival, DVec3::new(1.0, 64.0, 1.0));
        let mut data = placement(&mut world);
        let (tx, mut rx) = tokio::sync::mpsc::channel(16);
        world.entity_mut(player).insert((
            Rotation::default(),
            RuntimeEntityId(1),
            PlayerSession::new(1, "test".into(), None, None, tx),
        ));
        let mut actions = ActionQueue::default();
        actions.push(PluginAction::GiveItem {
            entity: player,
            item_id: "minecraft:stone".into(),
            count: 2,
        });
        world.insert_resource(actions);
        let mut schedule = Schedule::default();
        schedule.add_systems(super::super::plugins::process_plugin_actions);
        schedule.run(&mut world);
        let jolyne::valentine::McpePacketData::InventorySlotPacket(slot) =
            rx.try_recv().unwrap().data
        else {
            panic!("given slot")
        };
        data.item = slot.item;
        handle_block_click(&mut world, player, &data);
        let event = world.resource_mut::<EventBuffer>().drain().pop().unwrap();
        commit_block_event(&mut world, &event);
        assert_eq!(
            world
                .get::<ChunkData>(chunk)
                .unwrap()
                .inner
                .get_block(1, 65, 1),
            *blocks::STONE
        );
        assert_eq!(
            world
                .get::<MainInventory>(player)
                .unwrap()
                .0
                .item(0)
                .unwrap()
                .count,
            1
        );
    }
}
