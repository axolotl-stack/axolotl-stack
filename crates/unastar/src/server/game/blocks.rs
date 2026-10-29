//! Block domain system (exclusive).
//!
//! Processes block actions from PlayerAuthInput (break/place/crack) and
//! block clicks from InventoryTransaction. Runs as an exclusive system
//! because it needs `world.trigger()` and `world.write_message()`.

use bevy_ecs::prelude::*;
use glam::IVec3;
use tracing::{debug, trace, warn};

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
                    player_break_ticks(world, player_entity, (x, y, z))
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
                        player_break_ticks(world, player_entity, (x, y, z))
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

/// Runtime id of the block at `position`; `None` outside the world or an unloaded chunk.
pub(crate) fn block_at(world: &World, position: (i32, i32, i32)) -> Option<u32> {
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

/// What `use` does on an interactive block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BlockUse {
    Workbench,
    Container,
}

/// An interactive block's `use` (crafting table, container) runs before item
/// placement unless the player is sneaking with an item in hand.
fn block_use(world: &World, entity: Entity, clicked: (i32, i32, i32)) -> Option<BlockUse> {
    use crate::entity::components::{HeldSlot, MainInventory, PlayerState};
    let block_use = block_at(world, clicked).and_then(|runtime_id| {
        let entry = world
            .get_resource::<super::types::BlockRegistryResource>()?
            .0
            .get_by_runtime_id(runtime_id)?;
        if entry.string_id == "minecraft:crafting_table" {
            Some(BlockUse::Workbench)
        } else {
            crate::block_entity::ContainerKind::from_block(&entry.string_id)
                .map(|_| BlockUse::Container)
        }
    })?;
    let sneaking = world
        .get::<PlayerState>(entity)
        .is_some_and(|state| state.sneaking);
    let holding = world
        .get::<HeldSlot>(entity)
        .zip(world.get::<MainInventory>(entity))
        .and_then(|(slot, main)| main.0.item(usize::from(slot.0)))
        .is_some_and(|stack| !stack.is_empty());
    (!(sneaking && holding)).then_some(block_use)
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
    let item = stack.item()?;
    if stack.count > item.max_stack_size() {
        return None;
    }
    let block = world
        .get_resource::<super::types::BlockRegistryResource>()?
        .0
        .get_by_name(stack.item_id())?;
    Some((block.default_state_id, stack, item.network_id()))
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
            block_runtime_id: super::types::block_network_ids(world).to_wire(block_runtime_id),
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
            let upper = if can_interact(world, *entity, *position) {
                placement_upper(world, *entity, *position, *block_id)
            } else {
                None
            };
            let Some(upper) = upper else {
                reject_block_event(world, event);
                return;
            };
            place_block(world, position.0, position.1, position.2, *block_id);
            if let Some((above, upper_state)) = upper {
                place_block(world, above.x, above.y, above.z, upper_state);
            }
            super::containers::placed(world, IVec3::new(position.0, position.1, position.2));
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
    if block_id == *blocks::AIR
        || (!creative && player_break_ticks(world, player, position) == u32::MAX)
    {
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

    // A container hands its contents over before its actor goes with the block.
    let broken = super::containers::breaking(world, IVec3::new(x, y, z));

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

    super::falling::schedule_gravity_checks(world, IVec3::new(x, y, z));

    // Survival breaks drop the block's items as real item entities and wear
    // the tool.
    if original_block_id != *blocks::AIR
        && !world
            .get::<crate::entity::components::GameMode>(breaking_player)
            .is_some_and(|mode| mode.instant_break())
    {
        let tool = held_item(world, breaking_player);
        super::block_drops::drop_block_items(
            world,
            IVec3::new(x, y, z),
            original_block_id,
            tool.as_ref(),
        );
        wear_held_tool(world, breaking_player, original_block_id);
        super::hunger::exhaust_player(world, breaking_player, |values| values.mine);
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
    if let Some(broken) = broken {
        super::containers::drop_contents(world, IVec3::new(x, y, z), broken);
    }
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

    let ids = super::types::block_network_ids(world);
    let update_packet = UpdateBlockPacket {
        block_position: BlockPos { x, y, z },
        block_runtime_id: ids.to_wire(*blocks::AIR),
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
        data: ids.to_wire(original_block_id) as i32,
    };

    let sound_packet = LevelSoundEventPacket {
        sound_event: "break.block".into(),
        position: Vec3 {
            x: x as f32 + 0.5,
            y: y as f32 + 0.5,
            z: z as f32 + 0.5,
        },
        data: ids.to_wire(original_block_id) as i32,
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

/// The stack in the player's selected hotbar slot, if any.
fn held_item(world: &World, player: Entity) -> Option<crate::item::ItemStack> {
    let slot = world.get::<crate::entity::components::HeldSlot>(player)?.0;
    world
        .get::<crate::entity::components::MainInventory>(player)?
        .hotbar(slot)
        .filter(|stack| !stack.is_empty())
        .cloned()
}

/// Player state that affects break time, measured server-side: on ground
/// is a collision probe 1/16 block under the feet against block shapes
/// (not the client's flag), under water is the eye block being water.
fn mining_conditions(world: &World, player: Entity) -> super::mining::MiningConditions {
    use crate::entity::components::{Effects, PlayerState, Position};
    use crate::entity::systems::{BlockSource, collides_with_blocks};
    use crate::registry::collision::Aabb;
    let Some(eye) = world.get::<Position>(player).map(|p| p.0) else {
        return super::mining::MiningConditions::default();
    };
    let feet = eye - glam::DVec3::new(0.0, crate::entity::PLAYER_EYE_HEIGHT, 0.0);
    let probe = Aabb::new(
        feet + glam::DVec3::new(-0.3, -0.0625, -0.3),
        feet + glam::DVec3::new(0.3, 0.0, 0.3),
    );
    let underwater = world
        .block_at(eye.floor().as_ivec3())
        .zip(world.get_resource::<super::types::BlockRegistryResource>())
        .and_then(|(id, registry)| registry.0.get_by_runtime_id(id).map(|b| b.data))
        .is_some_and(|data| data.liquid && data.material == 5);
    super::mining::MiningConditions {
        on_ground: collides_with_blocks(world, probe),
        flying: world.get::<PlayerState>(player).is_some_and(|s| s.flying),
        riding: false,
        underwater,
        ..Default::default()
    }
    .with_effects(world.get::<Effects>(player))
}

/// Ticks `player` needs to break the block at `position` with the held item.
/// `u32::MAX` for unbreakable or unknown blocks.
fn player_break_ticks(world: &World, player: Entity, position: (i32, i32, i32)) -> u32 {
    let Some(block) = block_at(world, position).and_then(|id| {
        world
            .get_resource::<super::types::BlockRegistryResource>()?
            .0
            .get_by_runtime_id(id)
    }) else {
        return u32::MAX;
    };
    let tool = held_item(world, player);
    super::mining::break_ticks(block, tool.as_ref(), mining_conditions(world, player))
}

/// Apply mining wear to the held tool after breaking `block_runtime_id`
/// (shared durability rules; see `durability`).
fn wear_held_tool(world: &mut World, player: Entity, block_runtime_id: u32) {
    let Some(tool) = held_item(world, player) else {
        return;
    };
    let Some(block) = world
        .get_resource::<super::types::BlockRegistryResource>()
        .and_then(|r| r.0.get_by_runtime_id(block_runtime_id).cloned())
    else {
        return;
    };
    let wear = super::mining::mining_wear(&block, &tool);
    super::durability::wear_held(world, player, wear);
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
    match block_use(world, entity, clicked) {
        Some(BlockUse::Workbench) => {
            super::inventory::open_workbench(world, entity, clicked);
            return;
        }
        Some(BlockUse::Container) => {
            super::containers::open(world, entity, IVec3::new(clicked.0, clicked.1, clicked.2));
            return;
        }
        None => {}
    }
    // Spawn eggs spawn their mob on the clicked face instead of placing.
    let main_hand_selected = data.hand == jolyne::valentine::EnumsHandSlot::Mainhand
        && world
            .get::<crate::entity::components::HeldSlot>(entity)
            .is_some_and(|slot| data.slot == i32::from(slot.0));
    // Tools, bone meal, buckets and fire starters act on the clicked block
    // instead of placing anything.
    if main_hand_selected
        && super::item_use::on_block::use_on_block(
            world,
            entity,
            IVec3::new(clicked.0, clicked.1, clicked.2),
            data.face,
        )
    {
        return;
    }
    if main_hand_selected
        && block_at(world, clicked).is_some_and(|block| block != *blocks::AIR)
        && crate::entity::mob::interaction::interact::use_spawn_egg_on_block(
            world,
            entity,
            IVec3::new(position.0, position.1, position.2),
        )
    {
        return;
    }
    let held = held_block(world, entity);
    let block_ids = super::types::block_network_ids(world);
    let matches_held = held.as_ref().is_some_and(|(_, stack, network_id)| {
        data.hand == jolyne::valentine::EnumsHandSlot::Mainhand
            && world
                .get::<crate::entity::components::HeldSlot>(entity)
                .is_some_and(|slot| data.slot == i32::from(slot.0))
            && i32::from(data.item.id) == *network_id
            && data.item.stacksize == u16::from(stack.count)
            && u32::try_from(stack.damage).ok() == Some(data.item.auxvalue)
            && crate::item::user_data::stored_user_data(
                stack.item_id(),
                &data.item.user_data_buffer,
            ) == stack.nbt
            && u32::try_from(stack.runtime_id)
                .ok()
                .map(|id| block_ids.item_block_to_wire(i32::from(data.item.id), id))
                == Some(data.item.block_runtime_id)
    });
    let placement = held
        .as_ref()
        .filter(|_| matches_held)
        .and_then(|(held_state, _, _)| {
            let registry = &world
                .get_resource::<super::types::BlockRegistryResource>()?
                .0;
            let held_entry = registry.get_by_runtime_id(*held_state)?;
            let rotation = world
                .get::<crate::entity::components::Rotation>(entity)
                .copied()
                .unwrap_or_default();
            let input = super::placement::PlacementInput {
                clicked: IVec3::new(clicked.0, clicked.1, clicked.2),
                face: data.face,
                click: glam::DVec3::new(
                    f64::from(data.click_position.x),
                    f64::from(data.click_position.y),
                    f64::from(data.click_position.z),
                ),
                yaw: rotation.yaw,
                pitch: rotation.pitch,
            };
            super::placement::resolve_placement(world, held_entry, &input)
        });
    let Some(placement) = placement.filter(|placement| {
        let target = (
            placement.position.x,
            placement.position.y,
            placement.position.z,
        );
        can_interact(world, entity, clicked)
            && can_interact(world, entity, target)
            && block_at(world, clicked).is_some_and(|block| block != *blocks::AIR)
    }) else {
        correct_block(world, entity, clicked);
        correct_block(world, entity, position);
        super::inventory::resync_inventory(world, entity);
        return;
    };
    let target = (
        placement.position.x,
        placement.position.y,
        placement.position.z,
    );
    if placement_upper(world, entity, target, placement.state).is_none() {
        correct_block(world, entity, clicked);
        correct_block(world, entity, target);
        super::inventory::resync_inventory(world, entity);
        return;
    }
    let player_id = world
        .get::<PlayerUuid>(entity)
        .map(|id| id.0.to_string())
        .unwrap_or_default();
    if let Some(mut events) = world.get_resource_mut::<EventBuffer>() {
        events.push(ServerEvent::BlockPlace {
            entity,
            player_id,
            position: target,
            block_id: placement.state,
        });
    }
}

/// Validate placing `state` at `position` with the player's held block and
/// return the extra upper-half block (doors), `Some(None)` when there is
/// none, or `None` when the placement is not allowed (wrong item, occupied
/// target, entity in the way).
///
/// Runs both when the click is received and again at commit, after plugin
/// dispatch, so the world may have changed in between.
#[allow(clippy::option_option)]
fn placement_upper(
    world: &mut World,
    entity: Entity,
    position: (i32, i32, i32),
    state: u32,
) -> Option<Option<(IVec3, u32)>> {
    use super::placement::{double_slab, is_replaceable, obstructed, placed_boxes};
    let registry = world
        .get_resource::<super::types::BlockRegistryResource>()?
        .0
        .clone();
    let (held_state, _, _) = held_block(world, entity)?;
    let held = registry.get_by_runtime_id(held_state)?;
    let placed = registry.get_by_runtime_id(state)?;
    let pos = IVec3::new(position.0, position.1, position.2);
    let current_state = block_at(world, position)?;
    let current = registry.get_by_runtime_id(current_state)?;
    let merging_slab = double_slab(&registry, held).is_some_and(|double| {
        double.string_id == placed.string_id && current.string_id == held.string_id
    });
    let fills_free_space =
        placed.string_id == held.string_id && is_replaceable(current, current_state);
    if !(merging_slab || fills_free_space) {
        return None;
    }
    let upper = match placed.state_value(state, "upper_block_bit") {
        Some(crate::registry::block::StateValue::Byte(0)) => {
            let above = (position.0, position.1 + 1, position.2);
            let above_state = block_at(world, above)?;
            if !is_replaceable(registry.get_by_runtime_id(above_state)?, above_state) {
                return None;
            }
            let upper_state = placed.with_state_value(
                state,
                "upper_block_bit",
                crate::registry::block::StateValue::Byte(1),
            )?;
            Some((pos + IVec3::Y, upper_state))
        }
        Some(_) => return None,
        None => None,
    };
    let mut boxes = placed_boxes(&registry, pos, state);
    if let Some((above, upper_state)) = upper {
        boxes.extend(placed_boxes(&registry, above, upper_state));
    }
    if obstructed(world, &boxes) {
        return None;
    }
    Some(upper)
}

/// Place a block at world coordinates: update chunk and broadcast with the
/// place sound.
fn place_block(world: &mut World, x: i32, y: i32, z: i32, block_runtime_id: u32) {
    set_block(world, IVec3::new(x, y, z), block_runtime_id, true);
}

/// Set a block at world coordinates, broadcast the change to the chunk's
/// viewers (with the place sound when `place_sound`) and schedule gravity
/// checks for the block and the one above it.
pub(crate) fn set_block(world: &mut World, pos: IVec3, block_runtime_id: u32, place_sound: bool) {
    let (x, y, z) = (pos.x, pos.y, pos.z);
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

    super::falling::schedule_gravity_checks(world, pos);

    // Broadcast to viewers
    if let Some(chunk_viewers) = world.get::<ChunkViewers>(chunk_entity) {
        use jolyne::valentine::{LevelSoundEventPacket, UpdateBlockPacket};

        let wire_block = super::types::block_network_ids(world).to_wire(block_runtime_id);
        let update_packet = UpdateBlockPacket {
            block_position: BlockPos { x, y, z },
            block_runtime_id: wire_block,
            flags: BLOCK_UPDATE_NEIGHBOURS | BLOCK_UPDATE_NETWORK,
            layer: 0,
        };

        let sound_packet = place_sound.then(|| LevelSoundEventPacket {
            sound_event: "place".into(),
            position: Vec3 {
                x: x as f32 + 0.5,
                y: y as f32 + 0.5,
                z: z as f32 + 0.5,
            },
            data: wire_block as i32,
            actor_identifier: "".into(),
            is_baby: false,
            is_global: false,
            actor_unique_id: 0,
            fire_at_position: None,
        });

        for viewer_entity in chunk_viewers.iter() {
            if let Some(session) = world.get::<PlayerSession>(viewer_entity) {
                let _ = session.send(McpePacket::from(update_packet.clone()));
                if let Some(sound) = &sound_packet {
                    let _ = session.send(McpePacket::from(sound.clone()));
                }
            }
        }
    }
}

/// Break the block at `pos` without a breaker, as `/setblock … destroy` and
/// `/fill … destroy` do: viewers see the break particles, the block becomes
/// air, and its tool-less drops and any container contents fall as items
/// (BDS's order: `LevelEvent` 2001, `UpdateBlock`, `AddItemActor`).
pub(crate) fn destroy_block(world: &mut World, pos: IVec3) {
    let Some(original) = block_at(world, (pos.x, pos.y, pos.z)) else {
        return;
    };
    if original == *blocks::AIR {
        return;
    }
    let (cx, cz) = world_to_chunk_coords(pos.x, pos.z);
    let chunk = world
        .get_resource::<ChunkManager>()
        .and_then(|chunks| chunks.get_by_coords(cx, cz));
    if let Some(viewers) = chunk.and_then(|chunk| world.get::<ChunkViewers>(chunk)) {
        let particles = LevelEventPacket {
            event_id: LEVEL_EVENT_PARTICLES_DESTROY_BLOCK,
            position: Vec3 {
                x: pos.x as f32 + 0.5,
                y: pos.y as f32 + 0.5,
                z: pos.z as f32 + 0.5,
            },
            data: super::types::block_network_ids(world).to_wire(original) as i32,
        };
        for viewer in viewers.iter() {
            if let Some(session) = world.get::<PlayerSession>(viewer) {
                let _ = session.send(McpePacket::from(particles.clone()));
            }
        }
    }
    let broken = super::containers::breaking(world, pos);
    set_block(world, pos, *blocks::AIR, false);
    super::block_drops::drop_block_items(world, pos, original, None);
    if let Some(broken) = broken {
        super::containers::drop_contents(world, pos, broken);
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
        world.insert_resource(ChunkManager::new(
            crate::world::WorldConfig::void_spawn_platform(1),
        ));
        world.insert_resource(EventBuffer::default());
        world.insert_resource(super::super::types::ServerConfigResource(Default::default()));
        world.insert_resource(super::super::types::BlockRegistryResource(
            std::sync::Arc::new(crate::registry::BlockRegistry::vanilla().clone()),
        ));
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
    fn crafting_table_use_wins_unless_sneaking_with_an_item() {
        use crate::entity::components::PlayerState;
        let (mut world, player, chunk) =
            block_world(GameMode::Survival, DVec3::new(1.5, 65.62, 1.5));
        let table = crate::registry::BlockRegistry::vanilla()
            .default_state_id_by_name("minecraft:crafting_table")
            .unwrap();
        world
            .get_mut::<ChunkData>(chunk)
            .unwrap()
            .inner
            .set_block(1, 64, 1, table);
        world.entity_mut(player).insert(PlayerState::default());
        assert_eq!(
            block_use(&world, player, (1, 64, 1)),
            Some(BlockUse::Workbench)
        );
        world.get_mut::<PlayerState>(player).unwrap().sneaking = true;
        assert!(
            block_use(&world, player, (1, 64, 1)).is_some(),
            "sneaking, empty hand"
        );
        world
            .get_mut::<MainInventory>(player)
            .unwrap()
            .0
            .set_item(0, crate::item::test_support::stack("minecraft:stone", 1))
            .unwrap();
        assert!(
            block_use(&world, player, (1, 64, 1)).is_none(),
            "sneaking with an item places"
        );
        world.get_mut::<PlayerState>(player).unwrap().sneaking = false;
        assert_eq!(
            block_use(&world, player, (1, 64, 1)),
            Some(BlockUse::Workbench)
        );
        assert!(
            block_use(&world, player, (1, 65, 1)).is_none(),
            "air is not a table"
        );
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
        let id = items.get_by_name("minecraft:stone").unwrap().network_id();
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
            .set_item(0, crate::item::test_support::stack("minecraft:stone", 2))
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
    fn placing_inside_the_player_is_rejected() {
        let (mut world, player, chunk) = block_world(
            GameMode::Survival,
            DVec3::new(1.5, 65.0 + crate::entity::PLAYER_EYE_HEIGHT, 1.5),
        );
        world
            .entity_mut(player)
            .insert(crate::entity::components::Player);
        let data = placement(&mut world);
        world
            .get_mut::<MainInventory>(player)
            .unwrap()
            .0
            .set_item(0, crate::item::test_support::stack("minecraft:stone", 2))
            .unwrap();
        handle_block_click(&mut world, player, &data);
        assert!(
            !world
                .resource_mut::<EventBuffer>()
                .drain()
                .iter()
                .any(|event| matches!(event, ServerEvent::BlockPlace { .. }))
        );
        assert_eq!(
            world
                .get::<ChunkData>(chunk)
                .unwrap()
                .inner
                .get_block(1, 65, 1),
            *blocks::AIR
        );
    }

    #[test]
    fn doors_place_both_halves_facing_the_look_direction() {
        use crate::registry::block::StateValue;
        let (mut world, player, chunk) =
            block_world(GameMode::Survival, DVec3::new(3.5, 64.0, 1.5));
        let mut data = placement(&mut world);
        let door = crate::registry::BlockRegistry::vanilla()
            .get_by_name("minecraft:wooden_door")
            .unwrap();
        let items = world
            .resource::<super::super::types::ItemRegistryResource>()
            .0
            .clone();
        let door_item = items.get_by_name("minecraft:wooden_door").unwrap();
        let stack = crate::item::test_support::stack("minecraft:wooden_door", 1)
            .with_runtime_id(door.default_state_id as i32);
        data.item.id = door_item.network_id() as i16;
        data.item.stacksize = 1;
        data.item.block_runtime_id =
            crate::registry::block::BlockNetworkIds::Hashed.to_wire(door.default_state_id);
        world
            .get_mut::<MainInventory>(player)
            .unwrap()
            .0
            .set_item(0, stack)
            .unwrap();
        world
            .entity_mut(player)
            .insert(crate::entity::components::Rotation::new(270.0, 0.0));
        handle_block_click(&mut world, player, &data);
        let event = world.resource_mut::<EventBuffer>().drain().pop().unwrap();
        commit_block_event(&mut world, &event);
        let chunk = &world.get::<ChunkData>(chunk).unwrap().inner;
        let lower = chunk.get_block(1, 65, 1);
        let upper = chunk.get_block(1, 66, 1);
        assert_eq!(
            door.state_value(lower, "upper_block_bit"),
            Some(StateValue::Byte(0))
        );
        assert_eq!(
            door.state_value(upper, "upper_block_bit"),
            Some(StateValue::Byte(1))
        );
        assert_eq!(
            door.state_value(lower, "minecraft:cardinal_direction"),
            Some(StateValue::Str("east"))
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
            .set_item(0, crate::item::test_support::stack("minecraft:stone", 2))
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
                .set_item(0, crate::item::test_support::stack("minecraft:stone", 2))
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

    fn give(world: &mut World, player: Entity, stack: crate::item::ItemStack) {
        world
            .get_mut::<MainInventory>(player)
            .unwrap()
            .0
            .set_item(0, stack)
            .unwrap();
    }

    fn floor_under(world: &mut World, chunk: Entity) {
        let mut data = world.get_mut::<ChunkData>(chunk).unwrap();
        for x in 0..16 {
            for z in 0..16 {
                data.inner.set_block(x, 62, z, *blocks::STONE);
            }
        }
    }

    /// StartBreak timing comes from the vanilla formula with the held tool
    /// and the server-measured ground state, not a flat `hardness * 100`.
    #[test]
    fn break_time_uses_tool_and_ground_state() {
        let eye = DVec3::new(1.5, 63.0 + crate::entity::PLAYER_EYE_HEIGHT, 1.5);
        let (mut world, player, chunk) = block_world(GameMode::Survival, eye);
        floor_under(&mut world, chunk);
        assert_eq!(player_break_ticks(&world, player, (1, 64, 1)), 150);
        give(
            &mut world,
            player,
            crate::item::test_support::stack("minecraft:iron_pickaxe", 1),
        );
        assert_eq!(player_break_ticks(&world, player, (1, 64, 1)), 8);
        world
            .get_mut::<crate::entity::components::Position>(player)
            .unwrap()
            .0
            .y += 1.0;
        assert_eq!(
            player_break_ticks(&world, player, (1, 64, 1)),
            38,
            "airborne mining is five times slower"
        );
    }

    #[test]
    fn breaking_wears_the_tool_and_a_spent_tool_breaks() {
        use crate::item::enchant::{durability_damage, with_durability_damage};
        let eye = DVec3::new(1.5, 63.0 + crate::entity::PLAYER_EYE_HEIGHT, 1.5);
        let (mut world, player, chunk) = block_world(GameMode::Survival, eye);
        floor_under(&mut world, chunk);
        let mut items = crate::registry::ItemRegistry::new();
        items.load_vanilla();
        world.insert_resource(super::super::types::ItemRegistryResource(
            std::sync::Arc::new(items),
        ));
        give(
            &mut world,
            player,
            crate::item::test_support::stack("minecraft:iron_pickaxe", 1),
        );
        wear_held_tool(&mut world, player, *blocks::STONE);
        let held = held_item(&world, player).unwrap();
        assert_eq!(durability_damage(&held), 1);

        give(&mut world, player, with_durability_damage(&held, 249));
        wear_held_tool(&mut world, player, *blocks::STONE);
        assert!(
            held_item(&world, player).is_none(),
            "iron pickaxe has 250 uses"
        );
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
        let ids = crate::registry::block::BlockNetworkIds::Hashed;
        assert_eq!(clicked.block_runtime_id, ids.to_wire(*blocks::STONE));
        let McpePacketData::UpdateBlockPacket(placed) = rx.try_recv().unwrap().data else {
            panic!("predicted block correction")
        };
        assert_eq!(placed.block_position, BlockPos { x: 1, y: 65, z: 1 });
        assert_eq!(placed.block_runtime_id, ids.to_wire(*blocks::AIR));
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

    /// A cow spawn egg used on the top face of grass spawns a cow standing on
    /// that block, and uses one egg in survival but none in creative.
    #[test]
    fn cow_spawn_egg_on_grass_spawns_a_cow_on_top() {
        use crate::entity::mob::MobKind;
        for (mode, eggs_left) in [(GameMode::Survival, 1), (GameMode::Creative, 2)] {
            let mut server = super::super::GameServer::with_config(super::super::ServerConfig {
                world: crate::world::WorldConfig::void_spawn_platform(1),
                ..Default::default()
            });
            let world = server.ecs.world_mut();
            world.insert_resource(crate::entity::EntityRandom::seeded(3));
            let mut column = crate::world::chunk::Chunk::new(0, 0);
            for x in 0..16 {
                for z in 0..16 {
                    column.set_block(x, 63, z, *blocks::GRASS_BLOCK);
                }
            }
            let chunk = world.spawn(ChunkData::new(column)).id();
            world
                .resource_mut::<ChunkManager>()
                .insert(ChunkPosition::new(0, 0), chunk);
            let egg = super::super::world_stack(world, "minecraft:cow_spawn_egg", 2).unwrap();
            let egg_id = world
                .resource::<super::super::types::ItemRegistryResource>()
                .0
                .get_by_name("minecraft:cow_spawn_egg")
                .unwrap()
                .network_id();
            let mut inventory = MainInventory::default();
            inventory.0.set_item(0, egg).unwrap();
            let player = world
                .spawn((
                    Position(DVec3::new(
                        5.5,
                        64.0 + crate::entity::PLAYER_EYE_HEIGHT,
                        5.5,
                    )),
                    mode,
                    inventory,
                    HeldSlot(0),
                ))
                .id();
            let click = jolyne::valentine::ItemUseInventoryTransaction {
                position: BlockPos { x: 7, y: 63, z: 8 },
                face: 1,
                slot: 0,
                item: jolyne::valentine::CerealizerNetworkItemStackDescriptorSerializedData {
                    id: i16::try_from(egg_id).unwrap(),
                    stacksize: 2,
                    ..Default::default()
                },
                ..Default::default()
            };
            world
                .resource_mut::<BlockPacketQueue>()
                .0
                .push((player, BlockAction::BlockClick(click)));
            server.tick();

            let world = server.ecs.world_mut();
            let cows: Vec<DVec3> = world
                .query_filtered::<&Position, With<MobKind>>()
                .iter(world)
                .map(|position| position.0)
                .collect();
            assert_eq!(cows.len(), 1, "{mode:?}: one cow spawned");
            assert_eq!(cows[0].x, 7.5);
            assert_eq!(cows[0].z, 8.5);
            assert!(
                (64.0..64.01).contains(&cows[0].y),
                "{mode:?}: standing on the grass, at {}",
                cows[0].y
            );
            assert_eq!(
                world
                    .get::<MainInventory>(player)
                    .unwrap()
                    .0
                    .item(0)
                    .unwrap()
                    .count,
                eggs_left,
                "{mode:?}"
            );
            assert_eq!(
                world
                    .get::<ChunkData>(chunk)
                    .unwrap()
                    .inner
                    .get_block(7, 64, 8),
                *blocks::AIR,
                "nothing placed"
            );
        }
    }
}
