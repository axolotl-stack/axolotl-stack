//! Multi-player broadcasting system.
//!
//! ECS-based systems for broadcasting player spawn, movement, and despawn events
//! to all connected players. Uses proper batching via the network I/O layer.
//!
//! Uses spatial hashing (EntityGrid) for O(N) broadcast lookups instead of O(N²).

use bevy_ecs::prelude::*;
use jolyne::protocol::packets::PacketMovePlayerMode;
use jolyne::protocol::types::{
    AbilityLayers, AbilityLayersType, AbilitySet, CommandPermissionLevel, DeviceOs,
    EntityProperties, GameMode as ProtocolGameMode, Item, Links, McpePacket, MetadataDictionary,
    PermissionLevel, Vec3F,
};
use jolyne::protocol::{PacketAddPlayer, PacketMovePlayer, PacketRemoveEntity};
use std::collections::HashMap;
use uuid::Uuid;

use crate::entity::components::{
    GameMode, LastBroadcastPosition, PendingDespawnBroadcast, PendingSpawnBroadcast, Player,
    PlayerName, PlayerSession, PlayerUuid, Position, Rotation, RuntimeEntityId, SpatialChunk,
};

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
) -> PacketAddPlayer {
    let protocol_gamemode = match game_mode {
        GameMode::Survival => ProtocolGameMode::Survival,
        GameMode::Creative => ProtocolGameMode::Creative,
        GameMode::Adventure => ProtocolGameMode::Adventure,
        GameMode::Spectator => ProtocolGameMode::Creative, // No spectator in protocol
    };

    PacketAddPlayer {
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
) -> PacketMovePlayer {
    PacketMovePlayer {
        runtime_id: runtime_id as i32,
        position: Vec3F {
            x: position.0.x as f32,
            y: position.0.y as f32,
            z: position.0.z as f32,
        },
        pitch: rotation.pitch,
        yaw: rotation.yaw,
        head_yaw: rotation.yaw,
        mode: PacketMovePlayerMode::Normal,
        on_ground,
        ridden_runtime_id: 0,
        teleport: None,
        tick: 0,
    }
}

/// Builds a RemoveEntity packet for despawn broadcasting.
fn build_remove_entity_packet(runtime_id: i64) -> PacketRemoveEntity {
    PacketRemoveEntity {
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
/// Also inserts new players into the EntityGrid for spatial lookups.
/// Runs in NetworkSendSet after all spawn logic is complete.
pub fn broadcast_spawn_system(
    mut commands: Commands,
    mut grid: ResMut<EntityGrid>,
    new_players: Query<
        (
            Entity,
            &RuntimeEntityId,
            &PlayerUuid,
            &PlayerName,
            &Position,
            &Rotation,
            &GameMode,
            &PlayerSession,
            &SpatialChunk,
        ),
        With<PendingSpawnBroadcast>,
    >,
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
        (With<Player>, Without<PendingSpawnBroadcast>),
    >,
) {
    for (
        new_entity,
        new_rid,
        new_uuid,
        new_name,
        new_pos,
        new_rot,
        new_mode,
        new_session,
        spatial,
    ) in new_players.iter()
    {
        // Insert into EntityGrid for spatial lookups
        grid.insert(spatial.as_tuple(), new_entity);

        let new_packet = build_add_player_packet(
            new_rid.0,
            new_uuid.0,
            &new_name.0,
            new_pos,
            new_rot,
            *new_mode,
        );

        // Send new player to all existing players
        for (_, _, _, _, _, _, _, other_session) in existing_players.iter() {
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

        // Remove the pending marker and add LastBroadcastPosition
        commands
            .entity(new_entity)
            .remove::<PendingSpawnBroadcast>();
        commands.entity(new_entity).insert(LastBroadcastPosition {
            x: new_pos.0.x,
            y: new_pos.0.y,
            z: new_pos.0.z,
            yaw: new_rot.yaw,
            pitch: new_rot.pitch,
        });
    }
}

/// System: Update EntityGrid when players cross chunk boundaries.
///
/// Runs before broadcast systems to ensure spatial data is current.
pub fn update_spatial_grid_system(
    mut grid: ResMut<EntityGrid>,
    mut players: Query<(Entity, &Position, &mut SpatialChunk), With<Player>>,
) {
    for (entity, pos, mut tracker) in players.iter_mut() {
        let new_x = (pos.0.x.floor() as i32) >> 4;
        let new_z = (pos.0.z.floor() as i32) >> 4;

        if tracker.x != new_x || tracker.z != new_z {
            // Remove from old bucket
            grid.remove((tracker.x, tracker.z), entity);
            // Insert into new bucket
            grid.insert((new_x, new_z), entity);
            // Update tracker
            tracker.x = new_x;
            tracker.z = new_z;
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

/// System: Broadcast despawn for players marked for removal.
///
/// Runs in NetworkSendSet before CleanupSet removes the entities.
/// Also cleans up EntityGrid.
pub fn broadcast_despawn_system(
    mut grid: ResMut<EntityGrid>,
    despawning: Query<(Entity, &RuntimeEntityId, &SpatialChunk), With<PendingDespawnBroadcast>>,
    all_sessions: Query<(Entity, &PlayerSession), With<Player>>,
) {
    for (despawn_entity, rid, spatial) in despawning.iter() {
        // Remove from spatial grid
        grid.remove(spatial.as_tuple(), despawn_entity);

        let remove_packet = build_remove_entity_packet(rid.0);

        // Send to all OTHER players
        for (other_entity, other_session) in all_sessions.iter() {
            if other_entity != despawn_entity {
                let _ = other_session.send(McpePacket::from(remove_packet.clone()));
            }
        }
    }
}

// =============================================================================
// Block Breaking Systems
// =============================================================================

use crate::entity::components::BreakingState;
use crate::world::ChunkManager;
use crate::world::ecs::ChunkViewers;
use jolyne::protocol::packets::{PacketLevelEvent, PacketLevelEventEvent, PacketLevelSoundEvent};
use jolyne::protocol::types::SoundType;

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
    chunk_viewers: Query<&ChunkViewers>,
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

            let crack_event = PacketLevelEvent {
                event: PacketLevelEventEvent::BlockBreakSpeed,
                position: Vec3F {
                    x: x as f32,
                    y: y as f32,
                    z: z as f32,
                },
                data: event_data,
            };

            // Build punch particles (ParticlePunchBlock - generic, block-independent)
            let particle_event = PacketLevelEvent {
                event: PacketLevelEventEvent::ParticlePunchBlock,
                position: Vec3F {
                    x: x as f32 + 0.5,
                    y: y as f32 + 0.5,
                    z: z as f32 + 0.5,
                },
                data: 0, // We don't have block runtime ID readily available here
            };

            // Build breaking sound (SoundType::Hit with block data)
            let sound_event = PacketLevelSoundEvent {
                sound_id: SoundType::Hit,
                position: Vec3F {
                    x: x as f32 + 0.5,
                    y: y as f32 + 0.5,
                    z: z as f32 + 0.5,
                },
                extra_data: 0, // Block runtime ID - we don't have it here, but Hit still works
                entity_type: String::new(),
                is_baby_mob: false,
                is_global: false,
                entity_unique_id: 0,
            };

            // Broadcast to all chunk viewers including breaking player
            if let Ok(viewers) = chunk_viewers.get(chunk_entity) {
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
