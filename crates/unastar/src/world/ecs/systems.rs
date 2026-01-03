//! Chunk management systems for the ECS world.
//!
//! This module contains systems for:
//! - Processing chunk load queues (center-outward)
//! - Updating chunk viewers when players move
//! - Managing chunk unload with grace periods
//! - Sending chunk data to players
//! - Broadcasting block updates to viewers
//!
//! ## Performance Considerations
//! - Chunk ticking uses O(Players × SimDist²) not O(Chunks × Players)
//! - Player cleanup uses ChunkLoader's known set, not full world scan
//! - Publisher updates only sent on position/radius change, not per-chunk

use bevy_ecs::prelude::*;
use tracing::{debug, trace, warn};

use crate::entity::components::{ChunkRadius, Player, PlayerSession, Position};
use crate::world::ecs::{
    ChunkData, ChunkEntities, ChunkLoader, ChunkManager, ChunkPendingUnload, ChunkPosition,
    ChunkState, ChunkStateFlags, ChunkTickingState, ChunkViewers, PendingChunkGenerations,
};
use jolyne::valentine::types::{BlockCoordinates, UpdateBlockFlags};
use jolyne::valentine::{
    LevelChunkPacket, McpePacket, NetworkChunkPublisherUpdatePacket, UpdateBlockPacket,
};

/// Configuration for chunk loading behavior.
#[derive(Resource, Debug, Clone)]
pub struct ChunkLoadConfig {
    /// Maximum chunks to send per player per tick.
    pub chunks_per_tick: usize,
    /// Dimension ID for chunk packets.
    pub dimension: i32,
    /// Simulation distance in chunks.
    pub simulation_distance: i32,
    /// Grace period ticks before unloading chunks with no viewers.
    pub unload_grace_ticks: u32,
}

impl Default for ChunkLoadConfig {
    fn default() -> Self {
        Self {
            chunks_per_tick: 16, // Increased from 8 after generator optimization
            dimension: 0,
            simulation_distance: 6,
            unload_grace_ticks: 100,
        }
    }
}

impl ChunkLoadConfig {
    /// Create from ServerConfig.
    pub fn from_server_config(config: &crate::server::ServerConfig) -> Self {
        Self {
            chunks_per_tick: 8, // Low value to avoid slow ticks during terrain generation
            dimension: config.world.dimension,
            simulation_distance: config.simulation_distance,
            unload_grace_ticks: config.chunk_unload_ticks,
        }
    }
}

// NOTE: PlayerDisconnecting marker has been removed.
// ChunkLoader now has an on_remove hook that automatically cleans up
// chunk viewers when the component is removed during entity despawn.
// See loader.rs for the implementation.

/// Component tracking the last publisher position sent to client.
/// Used to avoid spamming NetworkChunkPublisherUpdate.
#[derive(Component, Debug, Default)]
pub struct LastPublisherState {
    pub chunk_x: i32,
    pub chunk_z: i32,
    pub radius: i32,
    /// True if we've sent the final update after queue emptied.
    pub queue_was_empty: bool,
}

// NOTE: initialize_chunk_loaders has been removed.
//
// ChunkLoader and LastPublisherState are now included in PlayerBundle at spawn time.
// This eliminates archetype changes that occurred when these components were
// added by a system after the player entity was spawned.
//
// The initialization logic that was here is now in GameServer::spawn_player().

/// System: Update ChunkLoaders when players move.
/// Evicts old chunks and queues new ones.
pub fn update_chunk_loaders(
    chunk_manager: Res<ChunkManager>,
    mut players: Query<
        (
            Entity,
            &Position,
            &ChunkRadius,
            &mut ChunkLoader,
            &mut LastPublisherState,
        ),
        With<Player>,
    >,
    mut chunks: Query<&mut ChunkViewers>,
) {
    for (player_entity, position, radius, mut loader, mut publisher_state) in players.iter_mut() {
        let chunk_x = (position.0.x / 16.0).floor() as i32;
        let chunk_z = (position.0.z / 16.0).floor() as i32;
        let current_pos = loader.position();

        trace!(
            player = ?player_entity,
            player_chunk = ?(chunk_x, chunk_z),
            loader_chunk = ?current_pos,
            "Update chunk loaders - checking position"
        );

        // Check if player crossed into a new chunk
        if current_pos == (chunk_x, chunk_z) {
            continue;
        }

        // Update loader position - this evicts out-of-range chunks
        let evicted = loader.move_to(chunk_x, chunk_z);

        // Handle radius changes
        if loader.radius() != radius.0 {
            let radius_evicted = loader.set_radius(radius.0);
            for pos in radius_evicted {
                if !evicted.contains(&pos) {
                    if let Some(chunk_entity) = chunk_manager.get_by_coords(pos.0, pos.1) {
                        if let Ok(mut viewers) = chunks.get_mut(chunk_entity) {
                            viewers.remove(player_entity);
                        }
                    }
                }
            }
        }

        // Remove player as viewer from evicted chunks
        for (ex, ez) in &evicted {
            if let Some(chunk_entity) = chunk_manager.get_by_coords(*ex, *ez) {
                if let Ok(mut viewers) = chunks.get_mut(chunk_entity) {
                    viewers.remove(player_entity);

                    trace!(
                        player = ?player_entity,
                        chunk = ?(ex, ez),
                        remaining_viewers = viewers.len(),
                        "Player left chunk view"
                    );
                }
            }
        }

        // NOTE: Don't update publisher_state here - let process_chunk_load_queues
        // detect the change and send the NetworkChunkPublisherUpdate
        publisher_state.queue_was_empty = false;

        trace!(
            player = ?player_entity,
            from = ?current_pos,
            to = ?(chunk_x, chunk_z),
            pending = loader.queue_len(),
            evicted = evicted.len(),
            "Player crossed chunk boundary - queuing new chunks"
        );
    }
}

/// System: Process chunk load queues and send chunks to players.
/// Sends up to N chunks per player per tick, ordered center-outward.
pub fn process_chunk_load_queues(
    config: Res<ChunkLoadConfig>,
    mut commands: Commands,
    mut chunk_manager: ResMut<ChunkManager>,
    mut players: Query<
        (
            Entity,
            &Position,
            &PlayerSession,
            &mut ChunkLoader,
            &mut LastPublisherState,
        ),
        With<Player>,
    >,
    mut chunks: Query<(&mut ChunkViewers, &ChunkData)>,
) {
    for (player_entity, position, session, mut loader, mut publisher_state) in players.iter_mut() {
        let sent_this_tick = loader.sent_this_tick;
        loader.sent_this_tick = 0; // Reset for this tick

        // Send publisher update if needed
        let chunk_x = (position.0.x.floor() as i32) >> 4;
        let chunk_z = (position.0.z.floor() as i32) >> 4;
        let radius = loader.radius();
        let publisher_radius = (radius as i32).saturating_mul(16);

        // Check if publisher update is needed
        let need_publisher_update = publisher_state.chunk_x != chunk_x
            || publisher_state.chunk_z != chunk_z
            || publisher_state.radius != publisher_radius;

        if need_publisher_update {
            debug!(player=?player_entity, old_pos = ?(publisher_state.chunk_x, publisher_state.chunk_z), new_pos = ?(chunk_x, chunk_z), "Player moved, sending NetworkChunkPublisherUpdate");
            let block_x = position.0.x.floor() as i32;
            let block_y = position.0.y.floor() as i32;
            let block_z = position.0.z.floor() as i32;

            if !session.send(McpePacket::from(NetworkChunkPublisherUpdatePacket {
                coordinates: BlockCoordinates {
                    x: block_x,
                    y: block_y,
                    z: block_z,
                },
                radius: publisher_radius,
                saved_chunks: vec![],
            })) {
                debug!(player = ?player_entity, "Failed to send NetworkChunkPublisherUpdate (channel full or closed)");
                continue;
            }

            publisher_state.chunk_x = chunk_x;
            publisher_state.chunk_z = chunk_z;
            publisher_state.radius = publisher_radius;
        }

        // If async generation is enabled, this system is a no-op for chunk loading.
        // request_chunk_generation handles everything for vanilla worlds.
        // We still update publisher state above, but skip chunk processing.
        if chunk_manager.has_async_generation() {
            let has_pending = loader.has_pending();
            if publisher_state.queue_was_empty != !has_pending {
                 debug!(player=?player_entity, has_pending, "Publisher state queue_was_empty updated");
            }
            publisher_state.queue_was_empty = !has_pending;
            continue;
        }

        // Non-async path (superflat, void, etc) - use synchronous get_or_create
        let mut sent_count = 0;
        let mut retry_count = 0;

        while sent_count < config.chunks_per_tick {
            let Some((cx, cz)) = loader.next_to_load() else {
                if sent_this_tick > 0 { // Log if we sent chunks last tick but not this one
                    debug!(player=?player_entity, "Chunk queue is now empty.");
                }
                break;
            };

            // Get or create chunk entity - returns (entity, Option<(encoded_biomes, highest_subchunk)>)
            // Phase 3: Now returns pre-encoded data to avoid ~200KB chunk clone
            // This also registers the player as a pending viewer in the ChunkManager
            let (chunk_entity, new_chunk_encoded) =
                chunk_manager.get_or_create(cx, cz, &mut commands, player_entity);

            // Track if this is a new chunk for logging
            let is_new_chunk = new_chunk_encoded.is_some();

            // Get chunk data from either the returned pre-encoded data or query the existing ChunkData component
            let chunk_data_result =
                if let Some((encoded_biomes, highest_subchunk)) = new_chunk_encoded {
                    // Newly created - use the pre-encoded data directly (Phase 3: no clone!)
                    Some((encoded_biomes, highest_subchunk))
                } else {
                    // Existing chunk - read from ChunkData component
                    chunks.get(chunk_entity).ok().map(|(_, chunk_data)| {
                        (
                            chunk_data.inner.encode_biomes(),
                            chunk_data.inner.highest_subchunk(),
                        )
                    })
                };

            let (payload, highest_subchunk) = match chunk_data_result {
                Some(data) => data,
                None => {
                    // ChunkData component not ready yet (commands not flushed)
                    // Leave in queue to retry next tick
                    trace!(
                        chunk = ?(cx, cz),
                        player = ?player_entity,
                        "ChunkData component not ready - will retry next tick"
                    );
                    retry_count += 1;

                    // If we've hit too many retries in a row, break to avoid infinite loop
                    // This can happen if chunks are being generated but components aren't flushing
                    if retry_count >= 3 {
                        debug!(
                            player = ?player_entity,
                            retry_count,
                            "Hit retry limit - breaking to allow command flush"
                        );
                        break;
                    }
                    continue;
                }
            };

            // Send LevelChunk packet
            let send_result = session.send(McpePacket::from(LevelChunkPacket {
                x: cx,
                z: cz,
                dimension: config.dimension,
                sub_chunk_count: crate::world::request_mode::LIMITED,
                highest_subchunk_count: Some(highest_subchunk),
                blobs: None,
                payload,
            }));

            if !send_result {
                debug!(
                    player = ?player_entity,
                    chunk = ?(cx, cz),
                    "Failed to send chunk packet (channel full or closed) - will retry next tick"
                );
                // Don't mark as loaded - leave in queue to retry
                // Break instead of continue to avoid burning through the whole queue on network errors
                break;
            }

            // ✅ ONLY mark as loaded after successful send
            loader.mark_loaded(cx, cz);
            sent_count += 1;
            loader.sent_this_tick += 1;
            retry_count = 0; // Reset retry counter on success

            trace!(
                chunk = ?(cx, cz),
                highest_subchunk = highest_subchunk,
                is_new = is_new_chunk,
                "Sent LevelChunk packet"
            );
        }

        // Track queue empty state for next tick
        publisher_state.queue_was_empty = !loader.has_pending();

        if sent_count > 0 {
            debug!(
                player = ?player_entity,
                sent = sent_count,
                remaining = loader.queue_len(),
                "Processed chunk queue this tick"
            );
        }

        // Diagnostic: Log if player has pending chunks but we couldn't send any
        if loader.has_pending() && sent_count == 0 {
            debug!(
                player = ?player_entity,
                pending_count = loader.queue_len(),
                retry_count,
                "Player has pending chunks but none were sent this tick (may need command flush or channel is full)"
            );
        }
    }
}

// =============================================================================
// Async Chunk Generation Systems (Phase 1 performance optimization)
// =============================================================================

/// System: Request async chunk generation for vanilla worlds.
///
/// This system handles the initial request phase for async generation:
/// - Pops chunks from player load queues
/// - For existing chunks: handles them immediately (send packet)
/// - For new chunks: submits generation requests to the background worker
/// - Tracks pending generations in `PendingChunkGenerations`
///
/// Non-blocking: generation happens in background, this just queues requests.
pub fn request_chunk_generation(
    mut chunk_manager: ResMut<ChunkManager>,
    mut pending_gens: ResMut<PendingChunkGenerations>,
    config: Res<ChunkLoadConfig>,
    mut players: Query<(Entity, &PlayerSession, &mut ChunkLoader), With<Player>>,
    chunks: Query<&ChunkData>,
) {
    // Skip if no async generation available (non-vanilla worlds)
    if !chunk_manager.has_async_generation() {
        return;
    }

    // Limit total pending generations to prevent memory growth
    const MAX_PENDING: usize = 64;

    for (player_entity, session, mut loader) in players.iter_mut() {
        let mut processed = 0;

        while processed < config.chunks_per_tick {
            if pending_gens.len() >= MAX_PENDING {
                debug!("Max pending chunk generations reached, skipping for now.");
                break;
            }

            let Some((cx, cz)) = loader.next_to_load() else {
                break; // No more chunks in this player's queue
            };

            // This is a critical log to see if we are processing the queue.
            trace!(player=?player_entity, chunk=?(cx, cz), "Popped chunk from load queue.");

            // Check if chunk already exists
            if let Some(chunk_entity) = chunk_manager.get_by_coords(cx, cz) {
                // Chunk exists - send it immediately
                if let Ok(chunk_data) = chunks.get(chunk_entity) {
                    let payload = chunk_data.inner.encode_biomes();
                    let highest_subchunk = chunk_data.inner.highest_subchunk();

                    let packet = LevelChunkPacket {
                        x: cx,
                        z: cz,
                        dimension: config.dimension,
                        sub_chunk_count: crate::world::request_mode::LIMITED,
                        highest_subchunk_count: Some(highest_subchunk),
                        blobs: None,
                        payload,
                    };

                    if session.send(McpePacket::from(packet)) {
                        loader.mark_loaded(cx, cz);
                        chunk_manager.pending_viewers.entry((cx, cz)).or_default().push(player_entity);
                        debug!(chunk = ?(cx, cz), player = ?player_entity, "Sent existing chunk to player.");
                    } else {
                        debug!(chunk = ?(cx, cz), player=?player_entity, "Failed to send existing chunk (channel full), will retry.");
                        loader.requeue_front(cx, cz); // Re-queue at the front to retry next tick
                        break; // Stop processing for this player if channel is full
                    }
                }
                processed += 1;
                continue;
            }

            // Skip if already pending generation
            if chunk_manager.is_generation_pending(cx, cz) {
                trace!(chunk = ?(cx, cz), "Chunk generation already pending, adding as viewer.");
                chunk_manager.pending_generation.get_mut(&(cx, cz)).map(|viewers| viewers.push(player_entity));
                loader.mark_loaded(cx, cz); // Mark as "loaded" to prevent re-requesting
                processed += 1;
                continue;
            }

            // Request async generation
            if let Some(receiver) = chunk_manager.request_generation(cx, cz, player_entity) {
                pending_gens.add(cx, cz, receiver);
                loader.mark_loaded(cx, cz); // Mark as loaded to prevent re-requesting
                processed += 1;
                debug!(player = ?player_entity, chunk = ?(cx, cz), "Requested async chunk generation.");
            }
        }
    }
}

/// System: Process completed async chunk generations.
pub fn process_completed_generations(
    mut commands: Commands,
    mut chunk_manager: ResMut<ChunkManager>,
    mut pending_gens: ResMut<PendingChunkGenerations>,
    config: Res<ChunkLoadConfig>,
    sessions: Query<&PlayerSession>,
    mut loaders: Query<&mut ChunkLoader>,
) {
    if pending_gens.is_empty() {
        return;
    }

    let mut completed = Vec::new();
    let initial_pending = pending_gens.len();

    // Check for completed generations (non-blocking)
    pending_gens.pending.retain_mut(|pending| {
        match pending.receiver.try_recv() {
            Ok(chunk) => {
                completed.push((pending.x, pending.z, chunk));
                false // Remove from pending
            }
            Err(tokio::sync::oneshot::error::TryRecvError::Empty) => true, // Keep
            Err(tokio::sync::oneshot::error::TryRecvError::Closed) => {
                warn!(chunk = ?(pending.x, pending.z), "Generation channel closed unexpectedly");
                let _ = chunk_manager.complete_generation(pending.x, pending.z);
                false // Remove from pending
            }
        }
    });
    
    if !completed.is_empty() {
        debug!(count = completed.len(), initial_pending, "Processing completed chunk generations.");
    }

    // Process completed chunks
    for (x, z, chunk) in completed {
        // Get viewers that were waiting for this chunk
        let viewers = chunk_manager.complete_generation(x, z).unwrap_or_default();

        // Encode BEFORE spawning to avoid clone
        let biome_data = chunk.encode_biomes();
        let highest_subchunk = chunk.highest_subchunk();

        // Spawn chunk entity
        let pos = ChunkPosition::new(x, z);
        let entity = commands.spawn((
            pos,
            ChunkData::new(chunk), // Move, not clone
            ChunkState::Loaded,
            ChunkViewers::default(),
            ChunkEntities::default(),
            ChunkStateFlags::new_generated(),
        )).id();

        chunk_manager.insert(pos, entity);

        debug!(chunk = ?(x, z), viewers = viewers.len(), "Async chunk generation complete, spawned entity.");

        // Send chunk to all waiting viewers
        for viewer_entity in viewers {
            if let Ok(session) = sessions.get(viewer_entity) {
                let packet = LevelChunkPacket {
                    x,
                    z,
                    dimension: config.dimension,
                    sub_chunk_count: crate::world::request_mode::LIMITED,
                    highest_subchunk_count: Some(highest_subchunk),
                    blobs: None,
                    payload: biome_data.clone(),
                };

                if session.send(McpePacket::from(packet)) {
                    // This was already marked as loaded on request, so we don't need to do it again.
                    debug!(chunk = ?(x, z), viewer = ?viewer_entity, "Sent async-generated chunk to viewer.");
                } else {
                     warn!(chunk = ?(x, z), viewer = ?viewer_entity, "Failed to send async-generated chunk (channel full).");
                     // We don't re-queue here because the loader thinks it's loaded.
                     // The client will hopefully re-request if it's missing.
                }
            }
            
            // Add viewer to the just-spawned chunk's ChunkViewers component
            chunk_manager.pending_viewers.entry((x, z)).or_default().push(viewer_entity);
        }
    }
}

/// System: Handle ChunkRadius changes.
/// When a player's chunk radius changes, update their loader.
pub fn handle_radius_changes(
    mut players: Query<
        (&ChunkRadius, &mut ChunkLoader, &mut LastPublisherState),
        (With<Player>, Changed<ChunkRadius>),
    >,
) {
    for (radius, mut loader, mut publisher_state) in players.iter_mut() {
        if loader.radius() != radius.0 {
            let evicted = loader.set_radius(radius.0);
            publisher_state.radius = radius.0;
            publisher_state.queue_was_empty = false;

            trace!(
                new_radius = radius.0,
                evicted_count = evicted.len(),
                "Player chunk radius changed"
            );
        }
    }
}

/// System: Schedule chunks for unload when they have no viewers.
/// Adds ChunkPendingUnload with grace period.
pub fn schedule_chunk_unloads(
    mut commands: Commands,
    config: Res<ChunkLoadConfig>,
    chunks: Query<
        (Entity, &ChunkPosition, &ChunkViewers),
        (With<ChunkState>, Without<ChunkPendingUnload>),
    >,
) {
    for (entity, pos, viewers) in chunks.iter() {
        if viewers.is_empty() {
            commands
                .entity(entity)
                .insert(ChunkPendingUnload::new(config.unload_grace_ticks));

            trace!(
                chunk = ?(pos.x, pos.z),
                grace_ticks = config.unload_grace_ticks,
                "Chunk scheduled for unload"
            );
        }
    }
}

/// System: Cancel chunk unload if viewers return.
pub fn cancel_chunk_unloads(
    mut commands: Commands,
    chunks: Query<(Entity, &ChunkPosition, &ChunkViewers), With<ChunkPendingUnload>>,
) {
    for (entity, pos, viewers) in chunks.iter() {
        if !viewers.is_empty() {
            commands.entity(entity).remove::<ChunkPendingUnload>();

            trace!(
                chunk = ?(pos.x, pos.z),
                viewers = viewers.len(),
                "Chunk unload cancelled - viewers returned"
            );
        }
    }
}

/// System: Process pending chunk unloads.
/// Ticks down grace period, saves modified chunks, and despawns when expired.
pub fn process_chunk_unloads(
    mut commands: Commands,
    mut chunk_manager: ResMut<ChunkManager>,
    mut chunks: Query<(
        Entity,
        &ChunkPosition,
        &mut ChunkPendingUnload,
        Option<&ChunkEntities>,
        Option<&ChunkData>,
        Option<&ChunkStateFlags>,
    )>,
) {
    for (entity, pos, mut pending, chunk_entities, chunk_data, state_flags) in chunks.iter_mut() {
        if pending.tick() {
            // Grace period expired - unload the chunk

            // First, freeze any non-player entities in the chunk
            if let Some(entities) = chunk_entities {
                if !entities.is_empty() {
                    trace!(
                        chunk = ?(pos.x, pos.z),
                        entity_count = entities.len(),
                        "TODO: Freeze entities before chunk unload"
                    );
                }
            }

            // Save modified chunks before despawning (check dirty flag)
            let is_dirty = state_flags.map(|f| f.is_dirty()).unwrap_or(false);
            if is_dirty {
                if let Some(chunk_data) = chunk_data {
                    if let Some(provider) = chunk_manager.provider() {
                        let chunk_pos = crate::world::ChunkPos::new(pos.x, pos.z);
                        let dim = chunk_manager.dimension();
                        let column = crate::storage::ChunkColumn::new(chunk_data.inner.clone());

                        // Block on async save (same pattern as load_or_generate_chunk)
                        if let Ok(handle) = tokio::runtime::Handle::try_current() {
                            let save_result = std::thread::spawn(move || {
                                handle.block_on(provider.save_column(chunk_pos, dim, &column))
                            })
                            .join();

                            match save_result {
                                Ok(Ok(())) => {
                                    debug!(chunk = ?(pos.x, pos.z), "Saved modified chunk before unload");
                                }
                                Ok(Err(e)) => {
                                    tracing::warn!(
                                        chunk = ?(pos.x, pos.z),
                                        error = %e,
                                        "Failed to save chunk before unload"
                                    );
                                }
                                Err(_) => {
                                    tracing::warn!(
                                        chunk = ?(pos.x, pos.z),
                                        "Thread panic while saving chunk"
                                    );
                                }
                            }
                        }
                    }
                }
            }

            // Remove from chunk manager
            chunk_manager.remove_by_coords(pos.x, pos.z);

            // Despawn the chunk entity
            commands.entity(entity).despawn();

            debug!(chunk = ?(pos.x, pos.z), "Chunk unloaded");
        }
    }
}

/// System: Flush pending viewers from ChunkManager to ECS components.
/// Ensures that even if a chunk was just spawned, its viewers are eventually synchronized.
pub fn flush_pending_viewers(
    mut chunk_manager: ResMut<ChunkManager>,
    mut chunks: Query<&mut ChunkViewers>,
) {
    if chunk_manager.is_empty() {
        return;
    }

    // We use a temporary vector to avoid borrowing issues while draining
    let mut still_pending = std::collections::HashMap::new();

    // Drain pending viewers and try to apply them to chunk entities
    let pending_items: Vec<_> = chunk_manager.pending_viewers.drain().collect();

    for (pos, viewers) in pending_items {
        if let Some(chunk_entity) = chunk_manager.get_by_coords(pos.0, pos.1) {
            if let Ok(mut viewers_comp) = chunks.get_mut(chunk_entity) {
                for viewer in viewers {
                    viewers_comp.insert(viewer);
                }
            } else {
                // Entity exists in manager but component not ready yet (likely spawned this tick)
                still_pending.insert(pos, viewers);
            }
        }
    }

    chunk_manager.pending_viewers = still_pending;
}

/// System: Update ChunkStateFlags::TICKING based on simulation distance.
///
/// OPTIMIZED (Phase 2): Uses bitflags mutation instead of component insert/remove,
/// avoiding all archetype thrashing. Reuses a persistent HashSet via ChunkTickingState
/// to eliminate per-tick allocations (~57KB/tick with many players).
pub fn update_chunk_ticking(
    config: Res<ChunkLoadConfig>,
    mut ticking_state: ResMut<ChunkTickingState>,
    players: Query<&Position, With<Player>>,
    mut chunks: Query<(&ChunkPosition, &mut ChunkStateFlags)>,
) {
    let sim_dist = config.simulation_distance;

    // Fast path: no players = clear all ticking flags
    if players.is_empty() {
        for (_, mut state) in chunks.iter_mut() {
            if state.is_ticking() {
                state.set_ticking(false);
            }
        }
        return;
    }

    // Clear and reuse the HashSet instead of allocating new
    // O(Players × SimDist²) - e.g., 50 players × 6² = 1,800 entries max
    ticking_state.should_tick.clear();

    for pos in players.iter() {
        let cx = (pos.0.x / 16.0).floor() as i32;
        let cz = (pos.0.z / 16.0).floor() as i32;

        for x in (cx - sim_dist)..=(cx + sim_dist) {
            for z in (cz - sim_dist)..=(cz + sim_dist) {
                ticking_state.should_tick.insert((x, z));
            }
        }
    }

    // Update chunk flags - no archetype changes!
    // Simply mutate the bitflag instead of inserting/removing components
    for (pos, mut state) in chunks.iter_mut() {
        let is_in_range = ticking_state.should_tick.contains(&pos.as_tuple());
        if state.is_ticking() != is_in_range {
            state.set_ticking(is_in_range);

            if !is_in_range {
                trace!(
                    chunk = ?(pos.x, pos.z),
                    "Chunk stopped ticking (outside sim distance)"
                );
            }
        }
    }
}

// NOTE: cleanup_disconnecting_player_views system has been removed.
// ChunkLoader's on_remove hook now handles cleanup automatically when
// the entity is despawned. This eliminates the need for the
// PlayerDisconnecting marker and the explicit cleanup system.

// ============================================================================
// Block Update Broadcasting
// ============================================================================

/// Broadcast a block update to all viewers of a chunk.
///
/// This should be called after a block is changed in the chunk data.
/// Sends `PacketUpdateBlock` to all players currently viewing the chunk.
///
/// # Arguments
/// * `chunk_entity` - The ECS entity of the chunk that was modified
/// * `world_x` - World X coordinate of the block
/// * `world_y` - World Y coordinate of the block  
/// * `world_z` - World Z coordinate of the block
/// * `block_runtime_id` - The new block runtime ID
/// * `viewers` - Query for chunk viewers
/// * `sessions` - Query for player sessions
#[allow(dead_code)]
pub fn broadcast_block_update(
    chunk_entity: Entity,
    world_x: i32,
    world_y: i32,
    world_z: i32,
    block_runtime_id: i32,
    viewers: &Query<&ChunkViewers>,
    sessions: &Query<&PlayerSession>,
) {
    let Ok(chunk_viewers) = viewers.get(chunk_entity) else {
        return;
    };

    let packet = UpdateBlockPacket {
        position: BlockCoordinates {
            x: world_x,
            y: world_y,
            z: world_z,
        },
        block_runtime_id,
        flags: UpdateBlockFlags::NEIGHBORS | UpdateBlockFlags::NETWORK,
        layer: 0,
    };

    for viewer_entity in chunk_viewers.iter() {
        if let Ok(session) = sessions.get(viewer_entity) {
            let _ = session.send(McpePacket::from(packet.clone()));
        }
    }

    trace!(
        chunk = ?chunk_entity,
        pos = ?(world_x, world_y, world_z),
        rid = block_runtime_id,
        viewers = chunk_viewers.len(),
        "Broadcast block update"
    );
}

/// Helper to compute chunk coordinates from world coordinates.
#[inline]
pub fn world_to_chunk_coords(world_x: i32, world_z: i32) -> (i32, i32) {
    (world_x >> 4, world_z >> 4)
}

/// Helper to compute local block coordinates from world coordinates.
#[inline]
pub fn world_to_local_coords(world_x: i32, world_y: i32, world_z: i32) -> (u8, i16, u8) {
    let local_x = (world_x & 15) as u8;
    let local_z = (world_z & 15) as u8;
    let local_y = world_y as i16;
    (local_x, local_y, local_z)
}

// ============================================================================
// Block Update Observer
// ============================================================================

use super::events::BlockChanged;

/// Observer for immediate reaction to block changes.
///
/// This observer fires synchronously within the same tick as the block change,
/// enabling immediate game logic reactions:
/// - Mark chunk dirty for persistence
/// - TODO: Check neighbor blocks for physics (sand falling, etc.)
/// - TODO: Update lighting
/// - TODO: Trigger redstone updates
///
/// Register with: `world.add_observer(on_block_changed)`
pub fn on_block_changed(trigger: On<BlockChanged>, mut chunks: Query<&mut ChunkStateFlags>) {
    let event = trigger.event();

    // Mark chunk dirty for persistence
    if let Ok(mut state) = chunks.get_mut(event.chunk_entity) {
        state.mark_dirty();
    }

    trace!(
        chunk = ?event.chunk_entity,
        pos = ?event.block_pos,
        old = event.old_block,
        new = event.new_block,
        "Block changed (observer)"
    );

    // TODO: Check neighbor blocks for physics (sand falling, etc.)
    // TODO: Update lighting
    // TODO: Trigger redstone updates
}

/// Plugin-like function to add all chunk systems to a schedule.
/// Call this during ECS setup.
///
/// NOTE: initialize_chunk_loaders was removed. ChunkLoader and LastPublisherState
/// are now included in PlayerBundle at spawn time to avoid archetype changes.
///
/// NOTE: cleanup_disconnecting_player_views was removed. ChunkLoader's on_remove
/// hook now handles cleanup automatically when player entities are despawned.
///
/// ## Async Generation (Phase 1 performance optimization)
///
/// For vanilla worlds, chunk generation is handled asynchronously:
/// - `request_chunk_generation` - Queues chunks for async generation (non-blocking)
/// - `process_completed_generations` - Spawns entities for completed generations
/// - `process_chunk_load_queues` - Handles sync cases (existing chunks, disk loads, superflat)
///
/// This prevents 300ms+ terrain generation from blocking the tick loop.
pub fn register_chunk_systems(schedule: &mut bevy_ecs::schedule::Schedule) {
    use crate::ecs::ChunkSet;

    schedule.add_systems(
        (
            // NOTE: initialize_chunk_loaders removed - components now in PlayerBundle
            update_chunk_loaders,
            flush_pending_viewers,
            request_chunk_generation,      // NEW: Non-blocking async request
            process_completed_generations, // NEW: Process async results
            process_chunk_load_queues,     // Handles sync cases + existing chunks
            handle_radius_changes,
            schedule_chunk_unloads,
            cancel_chunk_unloads,
            process_chunk_unloads,
            update_chunk_ticking,
        )
            .chain()
            .in_set(ChunkSet),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chunk_load_config_default() {
        let config = ChunkLoadConfig::default();
        assert_eq!(config.chunks_per_tick, 16); // Increased from 8 after generator optimization
        assert_eq!(config.dimension, 0);
        assert_eq!(config.simulation_distance, 6);
        assert_eq!(config.unload_grace_ticks, 100);
    }

    #[test]
    fn test_last_publisher_state_default() {
        let state = LastPublisherState::default();
        assert_eq!(state.chunk_x, 0);
        assert_eq!(state.chunk_z, 0);
        assert_eq!(state.radius, 0);
        assert!(!state.queue_was_empty);
    }
}

