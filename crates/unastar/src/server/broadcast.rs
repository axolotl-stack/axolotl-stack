//! Multi-player broadcasting system.
//!
//! ECS-based systems for broadcasting player spawn, movement, and despawn events
//! to all connected players. Uses proper batching via the network I/O layer.
//!
//! Uses spatial hashing (EntityGrid) for O(N) broadcast lookups instead of O(N²).

use bevy_ecs::prelude::*;
use jolyne::valentine::types::{
    AbilityLayers, AbilityLayersType, AbilitySet, CommandPermissionLevel, DeviceOs,
    EntityProperties, GameMode as ProtocolGameMode, Item, Links, MetadataDictionary,
    PermissionLevel, Vec3F,
};
use jolyne::valentine::{AddPlayerPacket, MovePlayerPacketMode, RemoveEntityPacket};
use jolyne::valentine::{McpePacket, MovePlayerPacket};
use std::collections::HashMap;
use uuid::Uuid;

use crate::entity::components::{
    GameMode, LastBroadcastPosition, Player, PlayerName, PlayerSession, PlayerUuid, Position,
    Rotation, RuntimeEntityId, SpatialChunk,
};
use crate::world::ecs::{PlayerDespawnedEvent, PlayerSpawnedEvent};

/// Spatial hash grid for efficient neighbor lookups.
/// Maps chunk coordinates to lists of entities in that chunk.
#[derive(Resource, Default)]
pub struct EntityGrid {
    buckets: HashMap<(i32, i32), Vec<Entity>>,
}

impl EntityGrid {
    /// Insert an entity into a chunk bucket.
    pub fn insert(&mut self, chunk: (i32, i32), entity: Entity) {
        self.buckets.entry(chunk).or_default().push(entity);
    }

    /// Remove an entity from a chunk bucket.
    pub fn remove(&mut self, chunk: (i32, i32), entity: Entity) {
        if let Some(bucket) = self.buckets.get_mut(&chunk) {
            bucket.retain(|&e| e != entity);
            if bucket.is_empty() {
                self.buckets.remove(&chunk);
            }
        }
    }

    /// Get all entities in neighboring chunks (square grid around center).
    pub fn get_neighbors(&self, center: (i32, i32), radius: i32) -> Vec<Entity> {
        let mut result = Vec::new();
        for dx in -radius..=radius {
            for dz in -radius..=radius {
                let chunk = (center.0 + dx, center.1 + dz);
                if let Some(bucket) = self.buckets.get(&chunk) {
                    result.extend(bucket.iter().copied());
                }
            }
        }
        result
    }
}

/// Builds an AddPlayer packet for broadcasting a player to others.
fn build_add_player_packet(
    runtime_id: i64,
    uuid: Uuid,
    name: &str,
    position: &Position,
    rotation: &Rotation,
    game_mode: GameMode,
) -> AddPlayerPacket {
    let protocol_gamemode = match game_mode {
        GameMode::Survival => ProtocolGameMode::Survival,
        GameMode::Creative => ProtocolGameMode::Creative,
        GameMode::Adventure => ProtocolGameMode::Adventure,
        GameMode::Spectator => ProtocolGameMode::Creative, // No spectator in protocol
    };

    AddPlayerPacket {
        uuid,
        username: name.to_string(),
        runtime_id,
        platform_chat_id: String::new(),
        position: Vec3F {
            x: position.0.x as f32,
            y: position.0.y as f32,
            z: position.0.z as f32,
        },
        velocity: Vec3F::default(),
        pitch: rotation.pitch,
        yaw: rotation.yaw,
        head_yaw: rotation.yaw,
        held_item: Item::default(),
        gamemode: protocol_gamemode,
        metadata: MetadataDictionary::default(),
        properties: EntityProperties::default(),
        unique_id: runtime_id,
        permission_level: PermissionLevel::Member,
        command_permission: CommandPermissionLevel::Normal,
        abilities: vec![AbilityLayers {
            type_: AbilityLayersType::Base,
            allowed: AbilitySet::all(),
            enabled: AbilitySet::BUILD | AbilitySet::MINE | AbilitySet::DOORS_AND_SWITCHES,
            fly_speed: 0.05,
            vertical_fly_speed: 0.05,
            walk_speed: 0.1,
        }],
        links: Links::default(),
        device_id: String::new(),
        device_os: DeviceOs::Undefined,
    }
}

/// Builds a MovePlayer packet for broadcasting position updates.
fn build_move_player_packet(
    runtime_id: i64,
    position: &Position,
    rotation: &Rotation,
    on_ground: bool,
) -> MovePlayerPacket {
    MovePlayerPacket {
        runtime_id: runtime_id as i32,
        position: Vec3F {
            x: position.0.x as f32,
            y: position.0.y as f32,
            z: position.0.z as f32,
        },
        pitch: rotation.pitch,
        yaw: rotation.yaw,
        head_yaw: rotation.yaw,
        mode: MovePlayerPacketMode::Normal,
        on_ground,
        ridden_runtime_id: 0,
        teleport: None,
        tick: 0,
    }
}

/// Builds a RemoveEntity packet for despawn broadcasting.
fn build_remove_entity_packet(runtime_id: i64) -> RemoveEntityPacket {
    RemoveEntityPacket {
        entity_id_self: runtime_id,
    }
}

/// Calculate squared distance for movement threshold.
fn position_distance_sq(last: &LastBroadcastPosition, current: &Position) -> f64 {
    let dx = last.x - current.0.x;
    let dy = last.y - current.0.y;
    let dz = last.z - current.0.z;
    dx * dx + dy * dy + dz * dz
}

/// Movement threshold squared (0.1 blocks)
const MOVEMENT_THRESHOLD_SQ: f64 = 0.01;

/// Rotation threshold (degrees)
const ROTATION_THRESHOLD: f32 = 1.0;

/// System: Broadcast newly spawned players to all existing players,
/// and send existing players to the new player.
///
/// Reads `PlayerSpawnedEvent` events instead of querying for marker components,
/// eliminating archetype changes when players spawn.
///
/// Note: EntityGrid insertion is handled by SpatialChunk's on_insert hook.
/// Runs in NetworkSendSet after all spawn logic is complete.
pub fn broadcast_spawn_system(
    mut events: MessageReader<PlayerSpawnedEvent>,
    // Query for new players by entity from the event
    new_players: Query<
        (
            &RuntimeEntityId,
            &PlayerUuid,
            &PlayerName,
            &Position,
            &Rotation,
            &GameMode,
            &PlayerSession,
        ),
        With<Player>,
    >,
    // Query for all existing players (to send to new player and to broadcast new player to them)
    existing_players: Query<
        (
            Entity,
            &RuntimeEntityId,
            &PlayerUuid,
            &PlayerName,
            &Position,
            &Rotation,
            &GameMode,
            &PlayerSession,
        ),
        With<Player>,
    >,
) {
    for event in events.read() {
        let new_entity = event.entity;

        // Get the new player's data
        let Ok((new_rid, new_uuid, new_name, new_pos, new_rot, new_mode, new_session)) =
            new_players.get(new_entity)
        else {
            // Entity may have been despawned between event emission and processing
            tracing::warn!(entity = ?new_entity, "PlayerSpawnedEvent for non-existent entity");
            continue;
        };

        // Note: EntityGrid insertion handled by SpatialChunk on_insert hook

        let new_packet = build_add_player_packet(
            new_rid.0,
            new_uuid.0,
            &new_name.0,
            new_pos,
            new_rot,
            *new_mode,
        );

        // Send new player to all existing players (except themselves)
        for (other_entity, _, _, _, _, _, _, other_session) in existing_players.iter() {
            if other_entity == new_entity {
                continue;
            }
            let _ = other_session.send(McpePacket::from(new_packet.clone()));
        }

        // Send all existing players to the new player
        for (
            other_entity,
            other_rid,
            other_uuid,
            other_name,
            other_pos,
            other_rot,
            other_mode,
            _,
        ) in existing_players.iter()
        {
            if other_entity == new_entity {
                continue;
            }
            let other_packet = build_add_player_packet(
                other_rid.0,
                other_uuid.0,
                &other_name.0,
                other_pos,
                other_rot,
                *other_mode,
            );
            let _ = new_session.send(McpePacket::from(other_packet));
        }

        // LastBroadcastPosition is now included in PlayerBundle at spawn time,
        // so no need to insert it here. This avoids an archetype change.
    }
}

/// System: Updates SpatialChunk when player crosses chunk boundaries.
///
/// Only runs on players with Changed<Position>, avoiding polling all players.
/// When SpatialChunk is mutated, the component's on_insert hook does NOT fire
/// (hooks only fire on insert/remove, not mutation), so we manually update
/// the EntityGrid here.
///
/// Runs before broadcast systems to ensure spatial data is current.
pub fn sync_spatial_chunks(
    mut grid: ResMut<EntityGrid>,
    mut players: Query<(Entity, &Position, &mut SpatialChunk), (With<Player>, Changed<Position>)>,
) {
    for (entity, pos, mut spatial) in players.iter_mut() {
        let new_x = (pos.0.x.floor() as i32) >> 4;
        let new_z = (pos.0.z.floor() as i32) >> 4;

        if spatial.x != new_x || spatial.z != new_z {
            // Remove from old bucket
            grid.remove((spatial.x, spatial.z), entity);
            // Insert into new bucket
            grid.insert((new_x, new_z), entity);
            // Update the component
            spatial.x = new_x;
            spatial.z = new_z;
        }
    }
}

/// System: Broadcast movement updates for players who have moved significantly.
///
/// Uses EntityGrid for O(N) spatial lookups instead of O(N²).
/// Only broadcasts if position changed by more than MOVEMENT_THRESHOLD or rotation changed.
pub fn broadcast_movement_system(
    grid: Res<EntityGrid>,
    mut players: Query<
        (
            Entity,
            &RuntimeEntityId,
            &Position,
            &Rotation,
            &mut LastBroadcastPosition,
        ),
        With<Player>,
    >,
    sessions: Query<(Entity, &PlayerSession)>,
) {
    // Collect movement updates to avoid borrow conflicts
    let mut updates: Vec<(Entity, i64, Position, Rotation, f64, f64, f64, f32, f32)> = Vec::new();

    let player_count = players.iter().count();
    for (entity, rid, pos, rot, last_pos) in players.iter() {
        let dist_sq = position_distance_sq(&last_pos, pos);
        let yaw_diff = (last_pos.yaw - rot.yaw).abs();
        let pitch_diff = (last_pos.pitch - rot.pitch).abs();

        if dist_sq > MOVEMENT_THRESHOLD_SQ
            || yaw_diff > ROTATION_THRESHOLD
            || pitch_diff > ROTATION_THRESHOLD
        {
            updates.push((
                entity,
                rid.0,
                pos.clone(),
                rot.clone(),
                pos.0.x,
                pos.0.y,
                pos.0.z,
                rot.yaw,
                rot.pitch,
            ));
        }
    }

    let update_count = updates.len();
    let mut packets_sent = 0usize;

    // Apply updates using spatial grid for O(N) lookups
    for (moving_entity, runtime_id, pos, rot, x, y, z, yaw, pitch) in updates {
        let move_packet = build_move_player_packet(runtime_id, &pos, &rot, true);
        let mover_chunk = ((pos.0.x.floor() as i32) >> 4, (pos.0.z.floor() as i32) >> 4);

        // O(1) neighbor lookup: get entities in 3x3 chunk area
        let nearby = grid.get_neighbors(mover_chunk, 1);

        for observer_entity in nearby {
            if observer_entity == moving_entity {
                continue;
            }
            if let Ok((_, session)) = sessions.get(observer_entity) {
                let _ = session.send(McpePacket::from(move_packet.clone()));
                packets_sent += 1;
            }
        }

        // Update last broadcast position
        if let Ok((_, _, _, _, mut last_pos)) = players.get_mut(moving_entity) {
            last_pos.x = x;
            last_pos.y = y;
            last_pos.z = z;
            last_pos.yaw = yaw;
            last_pos.pitch = pitch;
        }
    }

    // Log if there were any updates (avoid spam when idle)
    if update_count > 0 {
        tracing::trace!(
            players = player_count,
            updates = update_count,
            packets = packets_sent,
            "Broadcast movement"
        );
    }
}

/// System: Broadcast despawn for players being removed.
///
/// Reads `PlayerDespawnedEvent` events instead of querying for marker components,
/// eliminating archetype changes when players despawn.
///
/// Runs in NetworkSendSet before CleanupSet removes the entities.
/// Note: EntityGrid cleanup is handled by SpatialChunk's on_remove hook.
/// Note: ChunkViewers cleanup is handled by ChunkLoader's on_remove hook.
pub fn broadcast_despawn_system(
    mut events: MessageReader<PlayerDespawnedEvent>,
    all_sessions: Query<(Entity, &PlayerSession), With<Player>>,
) {
    for event in events.read() {
        let despawn_entity = event.entity;
        let runtime_id = event.runtime_id;

        // Note: EntityGrid removal handled by SpatialChunk on_remove hook
        // Note: ChunkViewers removal handled by ChunkLoader on_remove hook

        let remove_packet = build_remove_entity_packet(runtime_id);

        // Send to all OTHER players
        for (other_entity, other_session) in all_sessions.iter() {
            if other_entity != despawn_entity {
                let _ = other_session.send(McpePacket::from(remove_packet.clone()));
            }
        }
    }
}

/// System: Despawn entities that emitted PlayerDespawnedEvent.
///
/// Runs in CleanupSet after broadcast_despawn_system has sent packets.
/// Component on_remove hooks (SpatialChunk, ChunkLoader) handle cleanup automatically.
///
/// Note: We re-read the events here since we need to despawn the entities
/// after the broadcast system has processed them. Events are double-buffered,
/// so reading them in multiple systems within the same tick is safe.
pub fn cleanup_despawned_entities(
    mut commands: Commands,
    mut events: MessageReader<PlayerDespawnedEvent>,
) {
    for event in events.read() {
        commands.entity(event.entity).despawn();
    }
}

// =============================================================================
// Block Breaking Systems
// =============================================================================

use crate::entity::components::BreakingState;
use crate::world::ChunkManager;
use crate::world::ecs::{ChunkData, ChunkViewers};
use jolyne::valentine::types::SoundType;
use jolyne::valentine::{LevelEventPacket, LevelEventPacketEvent, LevelSoundEventPacket};

/// System: Tick block breaking state for all players.
///
/// For each player currently breaking a block:
/// - Increments their break counter
/// - Every 5 ticks: broadcasts cracking animation, punch particles, and breaking sound
///
/// This implements server-side block cracking animations like dragonfly does.
pub fn tick_block_breaking(
    mut players: Query<(Entity, &mut BreakingState, &PlayerSession), With<Player>>,
    chunk_manager: bevy_ecs::prelude::Res<ChunkManager>,
    chunks: Query<(&ChunkViewers, &ChunkData)>,
    all_sessions: Query<&PlayerSession>,
) {
    for (_player_entity, mut breaking, _player_session) in players.iter_mut() {
        // Skip if not breaking
        if !breaking.is_breaking() {
            continue;
        }

        let Some((x, y, z)) = breaking.position else {
            continue;
        };

        // Tick the breaking state - returns true every 5 ticks
        let should_broadcast = breaking.tick();

        if should_broadcast {
            // Calculate chunk coords
            let cx = x >> 4;
            let cz = z >> 4;

            // Get chunk entity to find viewers
            let Some(chunk_entity) = chunk_manager.get_by_coords(cx, cz) else {
                continue;
            };

            // Build LevelEvent::BlockBreakSpeed packet (cracking animation)
            let event_data = if breaking.expected_ticks > 0 {
                65535i32 / (breaking.expected_ticks as i32)
            } else {
                65535i32
            };

            let crack_event = LevelEventPacket {
                event: LevelEventPacketEvent::BlockBreakSpeed,
                position: Vec3F {
                    x: x as f32,
                    y: y as f32,
                    z: z as f32,
                },
                data: event_data,
            };

            // Look up block runtime ID from chunk data for correct particles/sounds
            let block_runtime_id = chunks
                .get(chunk_entity)
                .ok()
                .map(|(_, chunk_data)| {
                    let local_x = (x & 15) as u8;
                    let local_y = y as i16;
                    let local_z = (z & 15) as u8;
                    chunk_data.inner.get_block(local_x, local_y, local_z)
                })
                .unwrap_or(0);

            // Build punch particles with block runtime ID
            let particle_event = LevelEventPacket {
                event: LevelEventPacketEvent::ParticlePunchBlock,
                position: Vec3F {
                    x: x as f32 + 0.5,
                    y: y as f32 + 0.5,
                    z: z as f32 + 0.5,
                },
                data: block_runtime_id as i32,
            };

            // Build breaking sound (SoundType::Hit with block data)
            let sound_event = LevelSoundEventPacket {
                sound_id: SoundType::Hit,
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

            // Broadcast to all chunk viewers including breaking player
            if let Ok((viewers, _)) = chunks.get(chunk_entity) {
                for viewer_entity in viewers.iter() {
                    if let Ok(session) = all_sessions.get(viewer_entity) {
                        let _ = session.send(McpePacket::from(crack_event.clone()));
                        let _ = session.send(McpePacket::from(particle_event.clone()));
                        let _ = session.send(McpePacket::from(sound_event.clone()));
                    }
                }
            }

            tracing::debug!(
                pos = ?(x, y, z),
                counter = breaking.break_counter(),
                "Block cracking tick (with particles/sound)"
            );
        }
    }
}

// =============================================================================
// Batched Block Update Broadcasting
// =============================================================================

use crate::world::ecs::BlockBroadcastEvent;
use jolyne::valentine::UpdateBlockPacket;
use jolyne::valentine::types::UpdateBlockFlags;

/// System: Batch block updates and broadcast to chunk viewers.
///
/// Reads all `BlockBroadcastEvent` events from the current tick, groups them
/// by chunk entity, and sends batched `UpdateBlockPacket`s to viewers.
/// This reduces network overhead when multiple blocks change in the same tick.
///
/// Note: This system focuses on `UpdateBlockPacket` only. Particles and sounds
/// are still sent directly by `break_block`/`place_block` for now.
pub fn broadcast_block_updates(
    mut events: MessageReader<BlockBroadcastEvent>,
    chunks: Query<&ChunkViewers>,
    sessions: Query<&PlayerSession>,
) {
    // Group events by chunk for efficient packet bundling
    let mut updates_by_chunk: HashMap<Entity, Vec<BlockBroadcastEvent>> = HashMap::new();

    for event in events.read() {
        updates_by_chunk
            .entry(event.chunk_entity)
            .or_default()
            .push(event.clone());
    }

    // No events this tick
    if updates_by_chunk.is_empty() {
        return;
    }

    let mut total_packets_sent = 0usize;

    // Send batched updates to viewers
    for (chunk_entity, updates) in updates_by_chunk {
        let Ok(viewers) = chunks.get(chunk_entity) else {
            continue;
        };

        // Build packets for all updates in this chunk
        let packets: Vec<UpdateBlockPacket> = updates
            .iter()
            .map(|update| UpdateBlockPacket {
                position: jolyne::valentine::types::BlockCoordinates {
                    x: update.block_pos.x,
                    y: update.block_pos.y,
                    z: update.block_pos.z,
                },
                block_runtime_id: update.new_block as i32,
                flags: UpdateBlockFlags::NEIGHBORS | UpdateBlockFlags::NETWORK,
                layer: 0,
            })
            .collect();

        // Send all packets to each viewer
        for viewer in viewers.iter() {
            if let Ok(session) = sessions.get(viewer) {
                for packet in &packets {
                    let _ = session.send(McpePacket::from(packet.clone()));
                    total_packets_sent += 1;
                }
            }
        }
    }

    if total_packets_sent > 0 {
        tracing::trace!(
            packets = total_packets_sent,
            "Broadcast block updates (batched)"
        );
    }
}
