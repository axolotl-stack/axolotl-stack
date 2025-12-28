//! Block breaking and animation handling.
//!
//! Contains block breaking logic, crack animations, and hardness calculations.

use bevy_ecs::entity::Entity;
use tracing::{debug, info, trace};

use super::GameServer;
use crate::entity::components::{BreakingState, PlayerSession};
use crate::world::chunk::blocks;
use crate::world::ecs::{ChunkManager, ChunkViewers};
use crate::world::ecs::{world_to_chunk_coords, world_to_local_coords};
use jolyne::valentine::blocks::BLOCKS;
use jolyne::valentine::types::{Action, BlockCoordinates, Vec3F};
use jolyne::valentine::{LevelEventPacket, LevelEventPacketEvent, McpePacket};

/// Maximum block actions per PlayerAuthInput packet.
const MAX_BLOCK_ACTIONS: usize = 64;

impl GameServer {
    /// Handle block actions from PlayerAuthInput (block breaking, etc.)
    pub(super) fn handle_block_actions(
        &mut self,
        player_entity: Entity,
        pk: &jolyne::valentine::PlayerAuthInputPacket,
    ) {
        let Some(block_actions) = &pk.block_action else {
            return;
        };
        // Get current tick for timing
        let current_tick = self.current_tick;

        // Cap block actions per packet (DoS protection)
        for action_item in block_actions.iter().take(MAX_BLOCK_ACTIONS) {
            // Extract position from content if available
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

            match action_item.action {
                // Log ALL actions for debugging
                ref action => trace!(?action, "Block action received"),
            }

            match action_item.action {
                // Creative mode: instant block destroy
                Action::PredictBreak | Action::CreativePlayerDestroyBlock => {
                    if let Some((x, y, z)) = get_pos(&action_item.content) {
                        info!(pos = ?(x, y, z), "Creative/Predict block break");
                        self.break_block(player_entity, x, y, z);
                    }
                }
                // Survival or Creative: Start breaking - record state
                Action::StartBreak => {
                    // StartBreak might not have position content - get it from content if available
                    let pos = get_pos(&action_item.content);

                    if let Some((x, y, z)) = pos {
                        // Check if player is in creative mode (instant break)
                        let is_creative = {
                            let world = self.ecs.world();
                            world
                                .get::<crate::entity::components::GameMode>(player_entity)
                                .map(|gm| gm.instant_break())
                                .unwrap_or(false)
                        };

                        // Get actual break time from block hardness (0 for creative)
                        let break_time_ticks = if is_creative {
                            0 // Instant break in creative
                        } else {
                            self.get_block_break_time(x, y, z)
                        };

                        info!(pos = ?(x, y, z), is_creative, break_time_ticks, "StartBreak - setting break time");

                        // Update player breaking state
                        {
                            let world = self.ecs.world_mut();
                            if let Some(mut breaking) =
                                world.get_mut::<BreakingState>(player_entity)
                            {
                                breaking.start(x, y, z, current_tick, break_time_ticks);
                            }
                        }

                        // Broadcast crack animation to chunk viewers (except breaker)
                        if !is_creative {
                            self.broadcast_block_crack_start(x, y, z, break_time_ticks);
                        }
                    } else {
                        info!("StartBreak: no position in content (will use CrackBreak position)");
                    }
                }
                // CrackBreak: ongoing break animation - update position if StartBreak had none
                Action::CrackBreak | Action::ContinueBreak => {
                    if let Some((x, y, z)) = get_pos(&action_item.content) {
                        // If we're not already breaking, start breaking now
                        let needs_start = {
                            let world = self.ecs.world();
                            world
                                .get::<BreakingState>(player_entity)
                                .map(|b| b.position.is_none())
                                .unwrap_or(true)
                        };

                        if needs_start {
                            // Check if player is in creative mode
                            let is_creative = {
                                let world = self.ecs.world();
                                world
                                    .get::<crate::entity::components::GameMode>(player_entity)
                                    .map(|gm| gm.instant_break())
                                    .unwrap_or(false)
                            };

                            let break_time_ticks = if is_creative {
                                0
                            } else {
                                self.get_block_break_time(x, y, z)
                            };

                            info!(pos = ?(x, y, z), is_creative, break_time_ticks, "CrackBreak: starting break (StartBreak had no position)");

                            let world = self.ecs.world_mut();
                            if let Some(mut breaking) =
                                world.get_mut::<BreakingState>(player_entity)
                            {
                                breaking.start(x, y, z, current_tick, break_time_ticks);
                            }
                        }
                    }
                }
                // Survival mode: StopBreak means the client finished breaking
                // NOTE: StopBreak action has NO position data in content - use stored BreakingState.position
                // We trust the client timing since they have better info about tools, enchantments, haste, etc.
                Action::StopBreak => {
                    info!("StopBreak received");
                    // Get position from stored breaking state - trust client timing
                    let break_result = {
                        let world = self.ecs.world();
                        if let Some(breaking) = world.get::<BreakingState>(player_entity) {
                            if let Some((x, y, z)) = breaking.position {
                                let elapsed = current_tick.saturating_sub(breaking.start_tick);
                                info!(
                                    pos = ?(x, y, z),
                                    elapsed,
                                    expected = breaking.expected_ticks,
                                    "StopBreak - trusting client timing"
                                );
                                // Trust client - just ensure we have a valid position
                                Some((x, y, z))
                            } else {
                                info!("StopBreak: no breaking position stored");
                                None
                            }
                        } else {
                            info!("StopBreak: no BreakingState component");
                            None
                        }
                    };

                    if let Some((x, y, z)) = break_result {
                        info!(pos = ?(x, y, z), "Survival block break (StopBreak) - validated");
                        self.break_block(player_entity, x, y, z);

                        // Clear breaking state
                        let world = self.ecs.world_mut();
                        if let Some(mut breaking) = world.get_mut::<BreakingState>(player_entity) {
                            breaking.stop();
                        }

                        // Broadcast stop crack to all viewers
                        self.broadcast_block_crack_stop(x, y, z);
                    }
                }
                // AbortBreak: player stopped manually
                Action::AbortBreak => {
                    if let Some((x, y, z)) = get_pos(&action_item.content) {
                        debug!(pos = ?(x, y, z), "AbortBreak");

                        // Clear breaking state
                        let world = self.ecs.world_mut();
                        if let Some(mut breaking) = world.get_mut::<BreakingState>(player_entity) {
                            breaking.stop();
                        }

                        // Broadcast stop crack to all viewers
                        self.broadcast_block_crack_stop(x, y, z);
                    }
                }
                _ => {}
            }
        }
    }

    /// Broadcast block crack start animation to chunk viewers.
    /// Includes ALL viewers including the breaking player.
    pub(super) fn broadcast_block_crack_start(
        &self,
        x: i32,
        y: i32,
        z: i32,
        break_time_ticks: u32,
    ) {
        let (cx, cz) = world_to_chunk_coords(x, z);

        let world = self.ecs.world();
        let Some(chunk_manager) = world.get_resource::<ChunkManager>() else {
            return;
        };
        let Some(chunk_entity) = chunk_manager.get_by_coords(cx, cz) else {
            return;
        };

        // Calculate break speed data: 65535 / (break_time_seconds * 20)
        // break_time_ticks is already in ticks, so: 65535 / break_time_ticks
        let break_data = if break_time_ticks > 0 {
            (65535 / break_time_ticks) as i32
        } else {
            65535
        };

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
    /// Includes ALL viewers including the breaking player.
    pub(super) fn broadcast_block_crack_stop(&self, x: i32, y: i32, z: i32) {
        let (cx, cz) = world_to_chunk_coords(x, z);

        let world = self.ecs.world();
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
    ///
    /// The breaking_player receives the effect immediately, even if not yet in ChunkViewers.
    pub(super) fn break_block(&mut self, breaking_player: Entity, x: i32, y: i32, z: i32) {
        debug!(pos = ?(x, y, z), "break_block called");

        let (cx, cz) = world_to_chunk_coords(x, z);
        let (local_x, local_y, local_z) = world_to_local_coords(x, y, z);

        // Get chunk entity - must exist if player is viewing this chunk
        let chunk_entity = {
            let world = self.ecs.world();
            let Some(chunk_manager) = world.get_resource::<ChunkManager>() else {
                debug!("break_block: no ChunkManager resource");
                return;
            };
            chunk_manager.get_by_coords(cx, cz)
        };

        let Some(chunk_entity) = chunk_entity else {
            debug!(chunk = ?(cx, cz), "break_block: chunk entity not found - player mining in unloaded chunk?");
            return;
        };

        // Get the original block runtime ID before breaking (for particles/sound)
        let original_block_id = {
            let world = self.ecs.world();
            if let Some(chunk_data) = world.get::<crate::world::ecs::ChunkData>(chunk_entity) {
                chunk_data.inner.get_block(local_x, local_y, local_z)
            } else {
                0
            }
        };

        // Update block to air in chunk data (ECS component is source of truth)
        {
            let world = self.ecs.world_mut();
            if let Some(mut chunk_data) =
                world.get_mut::<crate::world::ecs::ChunkData>(chunk_entity)
            {
                chunk_data
                    .inner
                    .set_block(local_x, local_y, local_z, *blocks::AIR);
                // Log block ID with comparisons to known blocks
                let is_dirt = original_block_id == *blocks::DIRT;
                let is_grass = original_block_id == *blocks::GRASS_BLOCK;
                let is_stone = original_block_id == *blocks::STONE;
                info!(
                    local = ?(local_x, local_y, local_z),
                    original_block_id,
                    is_dirt,
                    is_grass,
                    is_stone,
                    dirt_id = *blocks::DIRT,
                    grass_id = *blocks::GRASS_BLOCK,
                    "break_block: set block to AIR"
                );
            } else {
                debug!(chunk = ?(cx, cz), "break_block: chunk data component not found");
                return;
            }

            // Mark chunk as modified for persistence
            world
                .entity_mut(chunk_entity)
                .insert(crate::world::ecs::ChunkModified);
        }

        // Spawn item drop if breaking player is in survival mode
        // and block is not air
        if original_block_id != *blocks::AIR {
            let is_survival = {
                let world = self.ecs.world();
                world
                    .get::<crate::entity::components::GameMode>(breaking_player)
                    .map(|gm| !gm.instant_break()) // Not creative/spectator = survival/adventure
                    .unwrap_or(true) // Default to survival
            };

            if is_survival {
                // Get item network ID for this block
                // Most blocks drop themselves - look up the item by block name
                let item_network_id = {
                    // Look up block name from runtime ID using jolyne BLOCKS slice
                    let block_name = BLOCKS.get(original_block_id as usize).map(|b| b.name());

                    use crate::registry::RegistryEntry;
                    block_name
                        .and_then(|name| self.items.get_by_name(name).map(|item| item.id()))
                        .unwrap_or(0)
                };

                if item_network_id > 0 {
                    // Allocate item entity ID
                    let item_entity_id = self.next_item_entity_id;
                    self.next_item_entity_id += 1;

                    // Create AddItemEntity packet
                    use jolyne::valentine::AddItemEntityPacket;
                    use jolyne::valentine::types::{
                        Item, ItemContent, ItemContentExtra, ItemExtraDataWithoutBlockingTick,
                    };

                    let item_packet = AddItemEntityPacket {
                        entity_id_self: item_entity_id,
                        runtime_entity_id: item_entity_id,
                        item: Item {
                            network_id: item_network_id as i32,
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
                            y: y as f32 + 0.25, // Spawn slightly above block center
                            z: z as f32 + 0.5,
                        },
                        velocity: Vec3F {
                            x: 0.0,
                            y: 0.1, // Small upward velocity
                            z: 0.0,
                        },
                        metadata: vec![], // Empty metadata
                        is_from_fishing: false,
                    };

                    // Send to breaking player
                    let world = self.ecs.world();
                    if let Some(session) = world.get::<PlayerSession>(breaking_player) {
                        let _ = session.send(McpePacket::from(item_packet.clone()));
                        info!(pos = ?(x, y, z), item_network_id, entity_id = item_entity_id, "Spawned item drop");
                    }

                    // Also send to chunk viewers
                    if let Some(chunk_viewers) = world.get::<ChunkViewers>(chunk_entity) {
                        for viewer in chunk_viewers.iter() {
                            if viewer != breaking_player {
                                if let Some(viewer_session) = world.get::<PlayerSession>(viewer) {
                                    let _ =
                                        viewer_session.send(McpePacket::from(item_packet.clone()));
                                }
                            }
                        }
                    }
                }
            }
        }

        // Broadcast to viewers
        let world = self.ecs.world();

        if let Some(chunk_viewers) = world.get::<ChunkViewers>(chunk_entity) {
            let viewer_count = chunk_viewers.len();
            debug!(viewer_count, "break_block: broadcasting UpdateBlock");

            // Prepare destroy particles and break sound
            use jolyne::valentine::types::{SoundType, UpdateBlockFlags};
            use jolyne::valentine::{
                LevelEventPacket, LevelEventPacketEvent, LevelSoundEventPacket, UpdateBlockPacket,
            };

            let update_packet = UpdateBlockPacket {
                position: BlockCoordinates { x, y, z },
                block_runtime_id: *blocks::AIR as i32,
                flags: UpdateBlockFlags::NEIGHBORS | UpdateBlockFlags::NETWORK,
                layer: 0,
            };

            // Destroy particles
            let particle_packet = LevelEventPacket {
                event: LevelEventPacketEvent::ParticleDestroy,
                position: Vec3F {
                    x: x as f32 + 0.5,
                    y: y as f32 + 0.5,
                    z: z as f32 + 0.5,
                },
                data: original_block_id as i32, // Block runtime ID for correct particle texture
            };

            // Break sound (SoundType::BreakBlock)
            let sound_packet = LevelSoundEventPacket {
                sound_id: SoundType::BreakBlock,
                position: Vec3F {
                    x: x as f32 + 0.5,
                    y: y as f32 + 0.5,
                    z: z as f32 + 0.5,
                },
                extra_data: original_block_id as i32, // Block runtime ID for correct sound
                entity_type: String::new(),
                is_baby_mob: false,
                is_global: false,
                entity_unique_id: 0,
            };

            // Track who we've sent to (to avoid duplicates if breaking player is in viewers)
            let mut sent_to = std::collections::HashSet::new();

            for viewer_entity in chunk_viewers.iter() {
                if let Some(session) = world.get::<PlayerSession>(viewer_entity) {
                    let _ = session.send(McpePacket::from(update_packet.clone()));
                    let _ = session.send(McpePacket::from(particle_packet.clone()));
                    let _ = session.send(McpePacket::from(sound_packet.clone()));
                    sent_to.insert(viewer_entity);
                }
            }

            // Always send to breaking player if not already sent
            if !sent_to.contains(&breaking_player) {
                if let Some(session) = world.get::<PlayerSession>(breaking_player) {
                    info!("Sending break effects directly to breaking player (not in viewers)");
                    let _ = session.send(McpePacket::from(update_packet.clone()));
                    let _ = session.send(McpePacket::from(particle_packet.clone()));
                    let _ = session.send(McpePacket::from(sound_packet.clone()));
                }
            }
        } else {
            // No chunk viewers - still send to breaking player

            use jolyne::valentine::types::{SoundType, UpdateBlockFlags};
            use jolyne::valentine::{
                LevelEventPacket, LevelEventPacketEvent, LevelSoundEventPacket, UpdateBlockPacket,
            };

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

            if let Some(session) = world.get::<PlayerSession>(breaking_player) {
                let _ = session.send(McpePacket::from(update_packet));
                let _ = session.send(McpePacket::from(particle_packet));
                let _ = session.send(McpePacket::from(sound_packet));
            }
        }
    }

    /// Get the break time in ticks for the block at the given world coordinates.
    /// Uses block hardness from BlockDefDyn. Formula: hardness * 1.5 * 20 ticks for bare hand.
    /// Returns minimum 1 tick, or 20 ticks if block not found.
    pub(super) fn get_block_break_time(&self, x: i32, y: i32, z: i32) -> u32 {
        let (cx, cz) = world_to_chunk_coords(x, z);
        let (local_x, local_y, local_z) = world_to_local_coords(x, y, z);

        // Get block runtime ID from chunk
        let block_runtime_id = {
            let world = self.ecs.world();
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

        // Find BlockDefDyn that contains this runtime ID
        for block_def in BLOCKS.iter() {
            let min = block_def.min_state_id();
            let max = block_def.max_state_id();
            if block_runtime_id >= min && block_runtime_id <= max {
                let hardness = block_def.hardness();
                // Bare hand break time: hardness * 5 seconds * 20 ticks/sec
                // TODO ONCE WE IMPLEMENT INVENTORY:
                // (Correct tool would be hardness * 1.5 * 20, but we assume bare hand)
                // For instant-break blocks (hardness 0), return 1 tick
                if hardness <= 0.0 {
                    return 1;
                }
                // Unbreakable blocks (hardness < 0) get max time
                if hardness < 0.0 {
                    return u32::MAX;
                }
                // Stone has hardness 1.5: 1.5 * 5 * 20 = 150 ticks = 7.5 seconds
                return (hardness * 5.0 * 20.0).ceil() as u32;
            }
        }

        // Block not found in registry, default 1 second
        20
    }
    /// Handle block click from ItemUse transaction
    pub(super) fn handle_block_click(
        &mut self,
        _entity: Entity,
        data: &jolyne::valentine::types::TransactionUseItem,
    ) {
        // 1. Get held item and map to block
        let network_id = data.held_item.network_id;
        if network_id == 0 {
            return; // Air, nothing to place
        }

        // Map item -> block
        // Note: O(n) scan for now, should be optimized with a HashMap lookup later
        let block_runtime_id = if let Some(item_entry) = self.items.get(network_id as u32) {
            if let Some(block_entry) = self.blocks.get_by_name(&item_entry.string_id) {
                // Log the mapping for debugging
                debug!(
                    "handle_block_click: Mapped item network_id={} string_id='{}' -> block id={} string_id='{}' default_state_id={} min={} max={}",
                    network_id,
                    item_entry.string_id,
                    block_entry.id,
                    block_entry.string_id,
                    block_entry.default_state_id,
                    block_entry.min_state_id,
                    block_entry.max_state_id
                );
                block_entry.default_state_id
            } else {
                debug!(
                    "handle_block_click: Item network_id={} string_id='{}' is not a block",
                    network_id, item_entry.string_id
                );
                return; // Item is not a block
            }
        } else {
            debug!("handle_block_click: Unknown item network_id={}", network_id);
            return; // Unknown item
        };

        // 2. Calculate placement position
        let mut x = data.block_position.x;
        let mut y = data.block_position.y;
        let mut z = data.block_position.z;

        match data.face {
            0 => y -= 1, // Down
            1 => y += 1, // Up
            2 => z -= 1, // North
            3 => z += 1, // South
            4 => x -= 1, // West
            5 => x += 1, // East
            _ => return, // Invalid face
        }

        // 3. Place block
        self.place_block(x, y, z, block_runtime_id);
    }

    /// Place a block at world coordinates: update chunk and broadcast
    pub(super) fn place_block(&mut self, x: i32, y: i32, z: i32, block_runtime_id: u32) {
        let (cx, cz) = world_to_chunk_coords(x, z);
        let (local_x, local_y, local_z) = world_to_local_coords(x, y, z);

        // Get chunk entity
        let world = self.ecs.world();
        let chunk_entity = if let Some(chunk_manager) = world.get_resource::<ChunkManager>() {
            chunk_manager.get_by_coords(cx, cz)
        } else {
            None
        };

        let Some(chunk_entity) = chunk_entity else {
            debug!(chunk = ?(cx, cz), "place_block: chunk not found");
            return;
        };

        // Update chunk data
        {
            let world = self.ecs.world_mut();
            if let Some(mut chunk_data) =
                world.get_mut::<crate::world::ecs::ChunkData>(chunk_entity)
            {
                chunk_data
                    .inner
                    .set_block(local_x, local_y, local_z, block_runtime_id);
            } else {
                return;
            }

            // Mark modified
            world
                .entity_mut(chunk_entity)
                .insert(crate::world::ecs::ChunkModified);
        }

        // Broadcast to viewers
        let world = self.ecs.world();
        if let Some(chunk_viewers) = world.get::<ChunkViewers>(chunk_entity) {
            use jolyne::valentine::types::{SoundType, UpdateBlockFlags};
            use jolyne::valentine::{LevelSoundEventPacket, UpdateBlockPacket};

            let update_packet = UpdateBlockPacket {
                position: BlockCoordinates { x, y, z },
                block_runtime_id: block_runtime_id as i32,
                flags: UpdateBlockFlags::NEIGHBORS | UpdateBlockFlags::NETWORK,
                layer: 0,
            };

            // Place sound
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
}
