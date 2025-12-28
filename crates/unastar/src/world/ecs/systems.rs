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
use tracing::{debug, trace};

use crate::entity::components::{ChunkRadius, Player, PlayerSession, Position};
use crate::world::ecs::{
    ChunkData, ChunkEntities, ChunkLoader, ChunkManager, ChunkModified, ChunkPendingUnload,
    ChunkPosition, ChunkState, ChunkTicking, ChunkViewers,
};
use jolyne::valentine::{
    LevelChunkPacket, NetworkChunkPublisherUpdatePacket, UpdateBlockPacket, McpePacket,
};
use jolyne::valentine::types::{BlockCoordinates, UpdateBlockFlags};

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
            chunks_per_tick: 4,
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
            chunks_per_tick: 4,
            dimension: config.world.dimension,
            simulation_distance: config.simulation_distance,
            unload_grace_ticks: config.chunk_unload_ticks,
        }
    }
}

/// Marker component for players that are disconnecting.
/// Added before despawn to allow cleanup systems to access their ChunkLoader.
#[derive(Component, Debug)]
pub struct PlayerDisconnecting;

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

/// System: Initialize ChunkLoader for new players.
/// Runs when a player spawns but doesn't have a ChunkLoader yet.
pub fn initialize_chunk_loaders(
    mut commands: Commands,
    players: Query<(Entity, &Position, &ChunkRadius), (With<Player>, Without<ChunkLoader>)>,
) {
    for (entity, position, radius) in players.iter() {
        let chunk_x = (position.0.x / 16.0).floor() as i32;
        let chunk_z = (position.0.z / 16.0).floor() as i32;

        let mut loader = ChunkLoader::new(radius.0);
        // Initialize loader at player's position and build queue
        loader.move_to(chunk_x, chunk_z);

        // If move_to was a no-op (same position), force reload
        if !loader.has_pending() {
            loader.force_reload();
        }

        commands.entity(entity).insert((
            loader,
            LastPublisherState {
                chunk_x,
                chunk_z,
                radius: radius.0,
                queue_was_empty: false,
            },
        ));

        trace!(
            entity = ?entity,
            chunk = ?(chunk_x, chunk_z),
            radius = radius.0,
            "Initialized ChunkLoader for player"
        );
    }
}

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
        (With<Player>, Changed<Position>),
    >,
    mut chunks: Query<&mut ChunkViewers>,
) {
    for (player_entity, position, radius, mut loader, mut publisher_state) in players.iter_mut() {
        let chunk_x = (position.0.x / 16.0).floor() as i32;
        let chunk_z = (position.0.z / 16.0).floor() as i32;
        let current_pos = loader.position();

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

        // Mark publisher state as needing update (position changed)
        publisher_state.chunk_x = chunk_x;
        publisher_state.chunk_z = chunk_z;
        publisher_state.queue_was_empty = false;

        trace!(
            player = ?player_entity,
            from = ?current_pos,
            to = ?(chunk_x, chunk_z),
            pending = loader.queue_len(),
            "Player moved to new chunk"
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
            let block_x = position.0.x.floor() as i32;
            let block_y = position.0.y.floor() as i32;
            let block_z = position.0.z.floor() as i32;

            if let Err(e) = session.send(McpePacket::from(NetworkChunkPublisherUpdatePacket {
                coordinates: BlockCoordinates {
                    x: block_x,
                    y: block_y,
                    z: block_z,
                },
                radius: publisher_radius,
                saved_chunks: vec![],
            })) {
                debug!(player = ?player_entity, error = ?e, "Failed to send NetworkChunkPublisherUpdate");
                continue;
            }

            publisher_state.chunk_x = chunk_x;
            publisher_state.chunk_z = chunk_z;
            publisher_state.radius = publisher_radius;
        }

        // Process up to N chunks from the queue
        let mut sent_count = 0;
        while sent_count < config.chunks_per_tick {
            let Some((cx, cz)) = loader.next_to_load() else {
                break;
            };

            // Get or create chunk entity - returns (entity, Option<newly_generated_chunk>)
            let (chunk_entity, new_chunk_data) = chunk_manager.get_or_create(cx, cz, &mut commands);

            // Add player as viewer if chunk already exists (new chunks need commands to flush first)
            if new_chunk_data.is_none() {
                if let Ok((mut viewers, _)) = chunks.get_mut(chunk_entity) {
                    viewers.insert(player_entity);
                }
            }

            // Get chunk data from either the returned new chunk or query the existing ChunkData component
            let (payload, highest_subchunk) = if let Some(ref chunk) = new_chunk_data {
                // Newly created - use the returned data directly
                (chunk.encode_biomes(), chunk.highest_subchunk())
            } else {
                // Existing chunk - read from ChunkData component
                if let Ok((_, chunk_data)) = chunks.get(chunk_entity) {
                    (
                        chunk_data.inner.encode_biomes(),
                        chunk_data.inner.highest_subchunk(),
                    )
                } else {
                    debug!(chunk = ?(cx, cz), "Failed to get ChunkData component");
                    continue;
                }
            };

            // Send LevelChunk packet
            if let Err(e) = session.send(McpePacket::from(LevelChunkPacket {
                x: cx,
                z: cz,
                dimension: config.dimension,
                sub_chunk_count: crate::world::request_mode::LIMITED,
                highest_subchunk_count: Some(highest_subchunk),
                blobs: None,
                payload,
            })) {
                debug!(
                    player = ?player_entity,
                    chunk = ?(cx, cz),
                    error = ?e,
                    "Failed to send chunk"
                );
                break;
            }

            // Mark as loaded in the loader
            loader.mark_loaded(cx, cz);
            sent_count += 1;

            trace!(
                player = ?player_entity,
                chunk = ?(cx, cz),
                "Sent chunk to player"
            );
        }

        // Track queue empty state for next tick
        publisher_state.queue_was_empty = !loader.has_pending();

        if sent_count > 0 {
            trace!(
                player = ?player_entity,
                sent = sent_count,
                remaining = loader.queue_len(),
                "Processed chunk queue"
            );
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
        Option<&ChunkModified>,
    )>,
) {
    for (entity, pos, mut pending, chunk_entities, chunk_data, modified) in chunks.iter_mut() {
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

            // Save modified chunks before despawning
            if modified.is_some() {
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

/// System: Update ChunkTicking markers based on simulation distance.
///
/// OPTIMIZED: Iterates players × SimDist² instead of chunks × players.
/// For 50 players with sim_dist=6: ~7,200 checks vs 250,000+ with old approach.
pub fn update_chunk_ticking(
    mut commands: Commands,
    config: Res<ChunkLoadConfig>,
    chunk_manager: Res<ChunkManager>,
    players: Query<&Position, With<Player>>,
    ticking_chunks: Query<(Entity, &ChunkPosition), With<ChunkTicking>>,
) {
    // Fast path: no players = no ticking chunks
    if players.is_empty() {
        // Remove ticking from all
        for (entity, _) in ticking_chunks.iter() {
            commands.entity(entity).remove::<ChunkTicking>();
        }
        return;
    }

    let sim_dist = config.simulation_distance;

    // Collect all chunks that SHOULD be ticking (set for dedup across players)
    // O(Players × SimDist²) - e.g., 50 players × 6² = 1,800 entries max
    let mut should_tick = std::collections::HashSet::new();

    for pos in players.iter() {
        let cx = (pos.0.x / 16.0).floor() as i32;
        let cz = (pos.0.z / 16.0).floor() as i32;

        for x in (cx - sim_dist)..=(cx + sim_dist) {
            for z in (cz - sim_dist)..=(cz + sim_dist) {
                should_tick.insert((x, z));
            }
        }
    }

    // Add ChunkTicking to chunks that should tick but don't have it
    for (x, z) in &should_tick {
        if let Some(chunk_entity) = chunk_manager.get_by_coords(*x, *z) {
            // try_insert avoids archetype fragmentation if already present
            commands.entity(chunk_entity).try_insert(ChunkTicking);
        }
    }

    // Remove ChunkTicking from chunks that are currently ticking but shouldn't be
    // O(currently_ticking_chunks) which is bounded by sim distance union across players
    for (entity, pos) in ticking_chunks.iter() {
        if !should_tick.contains(&(pos.x, pos.z)) {
            commands.entity(entity).remove::<ChunkTicking>();

            trace!(
                chunk = ?(pos.x, pos.z),
                "Chunk stopped ticking (outside sim distance)"
            );
        }
    }
}

/// System: Clean up chunk views when player is disconnecting.
/// Uses ChunkLoader's known set instead of full world scan.
///
/// OPTIMIZED: O(player's loaded chunks) instead of O(all chunks).
pub fn cleanup_disconnecting_player_views(
    mut commands: Commands,
    chunk_manager: Res<ChunkManager>,
    mut chunks: Query<&mut ChunkViewers>,
    disconnecting: Query<(Entity, &ChunkLoader), With<PlayerDisconnecting>>,
) {
    for (player_entity, loader) in disconnecting.iter() {
        // Only iterate chunks this player actually had loaded
        for (cx, cz) in loader.loaded_chunks() {
            if let Some(chunk_entity) = chunk_manager.get_by_coords(*cx, *cz) {
                if let Ok(mut viewers) = chunks.get_mut(chunk_entity) {
                    viewers.remove(player_entity);
                }
            }
        }

        trace!(
            player = ?player_entity,
            chunks_cleaned = loader.loaded_count(),
            "Cleaned up chunk views for disconnecting player"
        );

        // Now safe to fully despawn
        commands.entity(player_entity).despawn();
    }
}

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

/// Plugin-like function to add all chunk systems to a schedule.
/// Call this during ECS setup.
pub fn register_chunk_systems(schedule: &mut bevy_ecs::schedule::Schedule) {
    use crate::ecs::ChunkSet;

    schedule.add_systems(
        (
            initialize_chunk_loaders,
            update_chunk_loaders,
            process_chunk_load_queues,
            handle_radius_changes,
            schedule_chunk_unloads,
            cancel_chunk_unloads,
            process_chunk_unloads,
            update_chunk_ticking,
            cleanup_disconnecting_player_views,
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
        assert_eq!(config.chunks_per_tick, 4);
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
