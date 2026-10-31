//! Multi-player broadcasting system.
//!
//! ECS-based systems for broadcasting player spawn, movement, and despawn events
//! to all connected players. Uses proper batching via the network I/O layer.
//!
//! Uses spatial hashing for transient nearby events and reconciles per-viewer state.

pub mod player_views;

use bevy_ecs::prelude::*;
use jolyne::valentine::types::{
    ActorRuntimeId, ActorUniqueId, CerealizerNetworkItemStackDescriptorSerializedData,
    EnumsBuildPlatform, EnumsCommandPermissionLevel, EnumsGameType, EnumsPlayerPermissionLevel,
    EnumsPlayerPositionModeComponentPositionMode, PlayerInputTick, PropertySyncData,
    SerializedAbilitiesData, SerializedAbilitiesDataSerializedLayer,
    SynchedActorDataCopyableDataList, Vec2, Vec3,
};
use jolyne::valentine::{AddPlayerPacket, McpePacket, MovePlayerPacket, RemoveActorPacket};
use std::collections::HashMap;
use uuid::Uuid;

// Ability bit values and the base-layer ordinal are the protocol values used
// by Dragonfly/gophertunnel. Protocolgen exposes the generated fields but not
// these shared constants.
const ABILITY_BUILD: u32 = 1 << 0;
const ABILITY_MINE: u32 = 1 << 1;
const ABILITY_DOORS_AND_SWITCHES: u32 = 1 << 2;
const ABILITY_COUNT: u32 = 1 << 20;
const ABILITY_LAYER_BASE: u16 = 1;

use crate::entity::components::{
    GameMode, LastBroadcastPosition, Player, PlayerSession, Position, Rotation, RuntimeEntityId,
    SpatialChunk,
};
use crate::world::ecs::PlayerDespawnedEvent;

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
        GameMode::Survival => EnumsGameType::Survival,
        GameMode::Creative => EnumsGameType::Creative,
        GameMode::Adventure => EnumsGameType::Adventure,
        GameMode::Spectator => EnumsGameType::Creative, // No spectator in protocol
    };

    AddPlayerPacket {
        uuid,
        player_name: name.into(),
        target_runtime_id: ActorRuntimeId {
            actor_runtime_id: runtime_id as u64,
        },
        platform_chat_id: "".into(),
        position: Vec3 {
            x: position.0.x as f32,
            y: position.0.y as f32,
            z: position.0.z as f32,
        },
        velocity: Vec3::default(),
        rotation: Vec2 {
            x: rotation.pitch,
            y: rotation.yaw,
        },
        y_head_rotation: rotation.yaw,
        carried_item: CerealizerNetworkItemStackDescriptorSerializedData::default(),
        player_game_type: protocol_gamemode,
        entity_data: SynchedActorDataCopyableDataList::default(),
        synched_properties: PropertySyncData::default(),
        abilities_data: SerializedAbilitiesData {
            target_player_raw_id: runtime_id,
            player_permissions: EnumsPlayerPermissionLevel::Member,
            command_permissions: EnumsCommandPermissionLevel::Any,
            layers: vec![SerializedAbilitiesDataSerializedLayer {
                serialized_layer: ABILITY_LAYER_BASE,
                // AbilityCount - 1 from Dragonfly/gophertunnel: all defined
                // ability bits except the sentinel COUNT value.
                abilities_set: ABILITY_COUNT - 1,
                ability_values: ABILITY_BUILD | ABILITY_MINE | ABILITY_DOORS_AND_SWITCHES,
                fly_speed: 0.05,
                vertical_fly_speed: 0.05,
                walk_speed: 0.1,
            }],
        },
        actor_links: vec![],
        device_id: "".into(),
        build_platform: EnumsBuildPlatform::Unknown,
    }
}

/// Builds a MovePlayer packet for broadcasting position updates; `tick` is
/// the moving player's own input tick.
fn build_move_player_packet(
    runtime_id: i64,
    position: &Position,
    rotation: &Rotation,
    on_ground: bool,
    tick: PlayerInputTick,
) -> MovePlayerPacket {
    MovePlayerPacket {
        player_runtime_id: ActorRuntimeId {
            actor_runtime_id: runtime_id as u64,
        },
        position: Vec3 {
            x: position.0.x as f32,
            y: position.0.y as f32,
            z: position.0.z as f32,
        },
        rotation: Vec2 {
            x: rotation.pitch,
            y: rotation.yaw,
        },
        y_head_rotation: rotation.yaw,
        position_mode: EnumsPlayerPositionModeComponentPositionMode::Normal,
        on_ground,
        riding_runtime_id: ActorRuntimeId {
            actor_runtime_id: 0,
        },
        teleport_data: None,
        tick,
    }
}

/// Builds a RemoveActor packet for despawn broadcasting.
fn build_remove_actor_packet(runtime_id: i64) -> RemoveActorPacket {
    RemoveActorPacket {
        target_actor_id: ActorUniqueId {
            actor_unique_id: runtime_id,
        },
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

/// Reconcile player spawns, audiences, current equipment and movement.
/// Compatibility entry point for the complete player view system.
pub fn broadcast_spawn_system(world: &mut World) {
    player_views::sync_player_views(world);
}

/// System: Updates SpatialChunk when player crosses chunk boundaries.
///
/// Only runs on players with Changed<Position>, avoiding polling all players.
/// When SpatialChunk is mutated, the component's on_insert hook does NOT fire
/// (hooks only fire on insert/remove, not mutation), so we manually update
/// the EntityGrid here.
///
/// Runs before broadcast systems to ensure spatial data is current.
#[allow(clippy::type_complexity)]
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

/// Reconcile the complete player view after movement or audience changes.
pub fn broadcast_movement_system(world: &mut World) {
    player_views::sync_player_views(world);
}

/// Remove vanished/despawned actors from clients that actually knew them.
pub fn broadcast_despawn_system(world: &mut World) {
    player_views::sync_player_views(world);
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
use jolyne::valentine::{LevelEventPacket, LevelSoundEventPacket};

// Level event identifiers are the protocol constants used by Dragonfly and
// gophertunnel. Protocolgen 1.26.51 intentionally exposes this packet's event
// field as the canonical i32 rather than carrying the legacy enum wrapper.
const LEVEL_EVENT_BLOCK_BREAK_SPEED: i32 = 3602;
const LEVEL_EVENT_PARTICLE_PUNCH_BLOCK: i32 = 2014;
const BLOCK_UPDATE_NEIGHBOURS: u32 = 1;
const BLOCK_UPDATE_NETWORK: u32 = 1 << 1;

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
    config: Res<crate::server::game::types::ServerConfigResource>,
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
                event_id: LEVEL_EVENT_BLOCK_BREAK_SPEED,
                position: Vec3 {
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
            let block_runtime_id = config.0.block_network_ids.to_wire(block_runtime_id);

            // Build punch particles with block runtime ID
            let particle_event = LevelEventPacket {
                event_id: LEVEL_EVENT_PARTICLE_PUNCH_BLOCK,
                position: Vec3 {
                    x: x as f32 + 0.5,
                    y: y as f32 + 0.5,
                    z: z as f32 + 0.5,
                },
                data: block_runtime_id as i32,
            };

            // Build breaking sound (SoundType::Hit with block data)
            let sound_event = LevelSoundEventPacket {
                sound_event: "hit".into(),
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
    config: Res<crate::server::game::types::ServerConfigResource>,
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
                block_position: jolyne::valentine::types::BlockPos {
                    x: update.block_pos.x,
                    y: update.block_pos.y,
                    z: update.block_pos.z,
                },
                block_runtime_id: config.0.block_network_ids.to_wire(update.new_block),
                flags: BLOCK_UPDATE_NEIGHBOURS | BLOCK_UPDATE_NETWORK,
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

// =============================================================================
// Non-player actor visibility (items, falling blocks)
// =============================================================================

use crate::entity::components::{
    ActorViewers, FallingBlock, ItemEntityStack, ItemOwner, OnGround, PhysicsBody, RuntimeId,
    Velocity,
};
use crate::server::game::types::ItemRegistryResource;
use jolyne::valentine::types::{EnumsActorEvent, MoveActorAbsoluteData};
use jolyne::valentine::{
    ActorEventPacket, AddItemActorPacket, MoveActorAbsolutePacket, SetActorMotionPacket,
};

/// `MoveActorAbsoluteData` header bit: the actor is on the ground.
const MOVE_ACTOR_ON_GROUND: u8 = 0x01;

/// Squared position change (blocks) below which no move packet is sent.
const ACTOR_MOVE_EPSILON_SQ: f64 = 1e-8;

fn wire_vec(v: glam::DVec3) -> Vec3 {
    Vec3 {
        x: v.x as f32,
        y: v.y as f32,
        z: v.z as f32,
    }
}

/// Spawn packet for an actor, or `None` if this entity kind has no
/// network representation (or its item is not in the registry).
///
/// Falling blocks are `AddActor("minecraft:falling_block")` carrying the
/// block runtime ID in actor data `VARIANT` (2) and their 0.98 box in
/// `WIDTH`/`HEIGHT` (53/54), as dragonfly sends them.
#[allow(clippy::too_many_arguments)]
fn actor_spawn_packet(
    runtime_id: i64,
    wire_position: glam::DVec3,
    velocity: glam::DVec3,
    item: Option<(&ItemEntityStack, &ItemOwner)>,
    falling: Option<&FallingBlock>,
    body: &PhysicsBody,
    items: &crate::registry::ItemRegistry,
    block_ids: crate::registry::block::BlockNetworkIds,
) -> Option<McpePacket> {
    use jolyne::valentine::actor_data_i_ds as actor_data_ids;
    use jolyne::valentine::types::{
        DataItemEntry, DataItemEntryPayload, DataItemInt64Payload, EnumsDataItemType,
    };
    if let Some(falling) = falling {
        use jolyne::valentine::types::{DataItemFloatPayload, DataItemIntPayload};
        let float = |id, value| DataItemEntry {
            id,
            payload: DataItemEntryPayload::DataItemFloatPayload(DataItemFloatPayload {
                type_: EnumsDataItemType::Float,
                value,
            }),
        };
        return Some(McpePacket::from(jolyne::valentine::AddActorPacket {
            target_actor_id: ActorUniqueId {
                actor_unique_id: runtime_id,
            },
            target_runtime_id: ActorRuntimeId {
                actor_runtime_id: runtime_id as u64,
            },
            actor_type: "minecraft:falling_block".into(),
            position: wire_vec(wire_position),
            velocity: wire_vec(velocity),
            actor_data: SynchedActorDataCopyableDataList {
                data: vec![
                    DataItemEntry {
                        id: actor_data_ids::VARIANT,
                        payload: DataItemEntryPayload::DataItemIntPayload(DataItemIntPayload {
                            type_: EnumsDataItemType::Int,
                            value: block_ids.to_wire(falling.state) as i32,
                        }),
                    },
                    float(actor_data_ids::RESERVED_053, body.width as f32),
                    float(actor_data_ids::RESERVED_054, body.height as f32),
                ],
            },
            ..Default::default()
        }));
    }
    let (stack, owner) = item?;
    let item = crate::server::game::item_descriptor(&stack.0, items, block_ids)?;
    Some(McpePacket::from(AddItemActorPacket {
        target_actor_id: ActorUniqueId {
            actor_unique_id: runtime_id,
        },
        target_runtime_id: ActorRuntimeId {
            actor_runtime_id: runtime_id as u64,
        },
        item,
        position: wire_vec(wire_position),
        velocity: wire_vec(velocity),
        entity_data: SynchedActorDataCopyableDataList {
            data: vec![DataItemEntry {
                id: actor_data_ids::OWNER,
                payload: DataItemEntryPayload::DataItemInt64Payload(DataItemInt64Payload {
                    type_: EnumsDataItemType::Int64,
                    value: owner.0,
                }),
            }],
        },
        is_from_fishing: false,
    }))
}

/// System: keep every non-player actor spawned on exactly the clients that
/// view its chunk, and stream its movement to them.
///
/// New viewers get the spawn packet (`AddItemActor` for items, `AddActor`
/// for falling blocks and experience orbs), players that
/// no longer view the chunk get `RemoveActor`, and existing viewers get
/// `MoveActorAbsolute` + `SetActorMotion` when the position or velocity
/// changed and `ActorEvent(UpdateStackSize)` (event 69) when a merge or
/// partial pickup changed an item's count.
#[allow(clippy::type_complexity)]
pub fn sync_actor_viewers(
    chunk_manager: Res<ChunkManager>,
    chunk_viewers: Query<&ChunkViewers>,
    sessions: Query<&PlayerSession>,
    items: Res<ItemRegistryResource>,
    config: Res<crate::server::game::types::ServerConfigResource>,
    mut actors: Query<
        (
            Entity,
            &RuntimeId,
            &Position,
            &Velocity,
            &OnGround,
            &PhysicsBody,
            &mut ActorViewers,
            Option<&ItemEntityStack>,
            Option<&ItemOwner>,
            Option<&FallingBlock>,
            Option<&crate::entity::xp_orb::XpOrb>,
            Option<&crate::entity::components::ActorAudience>,
        ),
        // Mobs replicate through `entity::mob::net::network::sync_mob_viewers`.
        Without<crate::entity::components::Mob>,
    >,
) {
    for (
        actor,
        runtime_id,
        position,
        velocity,
        on_ground,
        body,
        mut viewers,
        stack,
        owner,
        falling,
        orb,
        audience,
    ) in actors.iter_mut()
    {
        let (cx, cz) = crate::world::ecs::world_to_chunk_coords(
            position.0.x.floor() as i32,
            position.0.z.floor() as i32,
        );
        let desired: Vec<Entity> = chunk_manager
            .get_by_coords(cx, cz)
            .and_then(|chunk| chunk_viewers.get(chunk).ok())
            .map(|v| {
                v.iter()
                    .filter(|e| {
                        sessions.contains(*e)
                            && audience.is_none_or(|audience| audience.allows(actor, *e))
                    })
                    .collect()
            })
            .unwrap_or_default();
        let wire_position = position.0 + glam::DVec3::new(0.0, body.network_y_offset, 0.0);

        let gone: Vec<Entity> = viewers
            .viewers
            .iter()
            .copied()
            .filter(|v| !desired.contains(v))
            .collect();
        if !gone.is_empty() {
            let remove = build_remove_actor_packet(runtime_id.0);
            viewers.viewers.retain(|viewer| {
                !gone.contains(viewer)
                    || sessions
                        .get(*viewer)
                        .is_ok_and(|session| !session.send(McpePacket::from(remove.clone())))
            });
        }

        let moved = position.0.distance_squared(viewers.last_position) > ACTOR_MOVE_EPSILON_SQ;
        let accelerated = velocity.0 != viewers.last_velocity;
        let count = stack.map_or(0, |s| s.0.count);
        let resized = stack.is_some() && count != viewers.last_stack_size;
        if (moved || accelerated || resized) && !viewers.viewers.is_empty() {
            let rid = ActorRuntimeId {
                actor_runtime_id: runtime_id.0 as u64,
            };
            let move_packet = moved.then(|| {
                McpePacket::from(MoveActorAbsolutePacket {
                    move_data: MoveActorAbsoluteData {
                        actor_runtime_id: rid.clone(),
                        header: if on_ground.0 { MOVE_ACTOR_ON_GROUND } else { 0 },
                        position: wire_vec(wire_position),
                        rotation_x: 0,
                        rotation_y: 0,
                        rotation_y_head: 0,
                    },
                })
            });
            let motion_packet = accelerated.then(|| {
                McpePacket::from(SetActorMotionPacket {
                    target_runtime_id: rid.clone(),
                    motion: wire_vec(velocity.0),
                    tick: PlayerInputTick { inputtick: 0 },
                })
            });
            let size_packet = resized.then(|| {
                McpePacket::from(ActorEventPacket {
                    target_runtime_id: rid.clone(),
                    event_id: EnumsActorEvent::UpdateStackSize,
                    data: i32::from(count),
                    fire_at_position: None,
                })
            });
            for viewer in viewers
                .viewers
                .iter()
                .filter(|viewer| desired.contains(viewer))
            {
                if let Ok(session) = sessions.get(*viewer) {
                    for packet in [&move_packet, &motion_packet, &size_packet]
                        .into_iter()
                        .flatten()
                    {
                        let _ = session.send(packet.clone());
                    }
                }
            }
        }

        let fresh: Vec<Entity> = desired
            .iter()
            .copied()
            .filter(|v| !viewers.viewers.contains(v))
            .collect();
        let spawn = || match orb {
            Some(orb) => Some(McpePacket::from(crate::entity::xp_orb::add_actor_packet(
                runtime_id.0,
                wire_position,
                velocity.0,
                orb,
            ))),
            None => actor_spawn_packet(
                runtime_id.0,
                wire_position,
                velocity.0,
                stack.zip(owner),
                falling,
                body,
                &items.0,
                config.0.block_network_ids,
            ),
        };
        if !fresh.is_empty()
            && let Some(spawn) = spawn()
        {
            for viewer in &fresh {
                if let Ok(session) = sessions.get(*viewer)
                    && session.send(spawn.clone())
                {
                    viewers.viewers.push(*viewer);
                }
            }
        }

        if moved {
            viewers.last_position = position.0;
        }
        if accelerated {
            viewers.last_velocity = velocity.0;
        }
        if resized {
            viewers.last_stack_size = count;
        }
    }
}

#[cfg(test)]
mod actor_viewer_tests {
    use super::*;
    use crate::entity::spawn_item_drop;
    use crate::world::ecs::ChunkPosition;
    use glam::DVec3;
    use jolyne::valentine::McpePacketData;
    use std::sync::Arc;

    fn setup() -> (
        World,
        Entity,
        Entity,
        tokio::sync::mpsc::Receiver<McpePacket>,
    ) {
        let mut world = World::new();
        let mut items = crate::registry::ItemRegistry::new();
        items.load_vanilla();
        world.insert_resource(ItemRegistryResource(Arc::new(items)));
        world.insert_resource(crate::server::game::types::ServerConfigResource(
            Default::default(),
        ));
        world.insert_resource(ChunkManager::new(
            crate::world::WorldConfig::void_spawn_platform(1),
        ));
        let chunk = world
            .spawn(ChunkData::new(crate::world::chunk::Chunk::new(0, 0)))
            .id();
        world
            .resource_mut::<ChunkManager>()
            .insert(ChunkPosition::new(0, 0), chunk);
        let (tx, rx) = tokio::sync::mpsc::channel(64);
        let player = world
            .spawn(PlayerSession::new(1, "viewer".into(), None, None, tx))
            .id();
        let mut viewers = ChunkViewers::default();
        viewers.insert(player);
        world.entity_mut(chunk).insert(viewers);
        (world, chunk, player, rx)
    }

    fn sync(world: &mut World) {
        let mut schedule = Schedule::default();
        schedule.add_systems(sync_actor_viewers);
        schedule.run(world);
    }

    fn drain(rx: &mut tokio::sync::mpsc::Receiver<McpePacket>) -> Vec<McpePacketData> {
        std::iter::from_fn(|| rx.try_recv().ok().map(|p| p.data)).collect()
    }

    #[test]
    fn falling_blocks_spawn_as_falling_block_actors_with_their_state() {
        let (mut world, _, _, mut rx) = setup();
        let sand = crate::registry::BlockRegistry::vanilla()
            .default_state_id_by_name("minecraft:sand")
            .unwrap();
        world.spawn((
            FallingBlock { state: sand },
            Position(DVec3::new(4.5, 70.0, 4.5)),
            Velocity::default(),
            OnGround(false),
            PhysicsBody::FALLING_BLOCK,
            RuntimeId(7),
            ActorViewers::default(),
        ));
        sync(&mut world);
        let packets = drain(&mut rx);
        let [McpePacketData::AddActorPacket(add)] = packets.as_slice() else {
            panic!("expected AddActor, got {packets:?}");
        };
        assert_eq!(&*add.actor_type, "minecraft:falling_block");
        assert_eq!(add.position.y, 70.49);
        assert!(add.actor_data.data.iter().any(|entry| entry.id == 2
            && matches!(&entry.payload,
                jolyne::valentine::types::DataItemEntryPayload::DataItemIntPayload(v)
                    if v.value == crate::registry::block::BlockNetworkIds::Hashed.to_wire(sand) as i32)));
    }

    #[test]
    fn rejected_spawn_is_not_recorded_as_visible_and_reconciles_after_backpressure() {
        let (mut world, _, player, mut rx) = setup();
        let session = world.get::<PlayerSession>(player).unwrap();
        let filler = McpePacket::from(build_remove_actor_packet(99));
        while session.send(filler.clone()) {}
        let sand = crate::registry::BlockRegistry::vanilla()
            .default_state_id_by_name("minecraft:sand")
            .unwrap();
        let actor = world
            .spawn((
                FallingBlock { state: sand },
                Position(DVec3::new(4.5, 70.0, 4.5)),
                Velocity::default(),
                OnGround(false),
                PhysicsBody::FALLING_BLOCK,
                RuntimeId(7),
                ActorViewers::default(),
            ))
            .id();
        sync(&mut world);
        assert!(
            world.get::<ActorViewers>(actor).unwrap().viewers.is_empty(),
            "a rejected AddActor must not make an actor visible"
        );
        drain(&mut rx);
        sync(&mut world);
        let packets = drain(&mut rx);
        let [McpePacketData::AddActorPacket(add)] = packets.as_slice() else {
            panic!("expected deferred AddActor, got {packets:?}");
        };
        assert_eq!(add.target_runtime_id.actor_runtime_id, 7);
        assert_eq!(world.get::<ActorViewers>(actor).unwrap().viewers, [player]);
        sync(&mut world);
        assert!(drain(&mut rx).is_empty(), "accepted spawn must not repeat");
    }

    #[test]
    fn actor_audience_hides_and_reveals_existing_item_actors() {
        let (mut world, _, viewer, mut rx) = setup();
        let item = spawn_item_drop(
            &mut world,
            DVec3::new(4.5, 64.0, 4.5),
            crate::item::test_support::stack("minecraft:dirt", 1),
            None,
        )
        .unwrap();
        world
            .entity_mut(item)
            .insert(crate::entity::components::ActorAudience::only([]));
        sync(&mut world);
        assert!(drain(&mut rx).is_empty());
        world
            .entity_mut(item)
            .insert(crate::entity::components::ActorAudience::only([viewer]));
        sync(&mut world);
        assert!(matches!(
            drain(&mut rx).as_slice(),
            [McpePacketData::AddItemActorPacket(_)]
        ));
        world
            .entity_mut(item)
            .insert(crate::entity::components::ActorAudience::only([]));
        sync(&mut world);
        assert!(matches!(
            drain(&mut rx).as_slice(),
            [McpePacketData::RemoveActorPacket(_)]
        ));
    }

    #[test]
    fn items_spawn_move_resize_and_leave_with_their_chunk_viewers() {
        let (mut world, chunk, player, mut rx) = setup();
        let item = spawn_item_drop(
            &mut world,
            DVec3::new(4.5, 64.0, 4.5),
            crate::item::test_support::stack("minecraft:dirt", 2),
            None,
        )
        .unwrap();
        sync(&mut world);
        let packets = drain(&mut rx);
        let [McpePacketData::AddItemActorPacket(add)] = packets.as_slice() else {
            panic!("expected one AddItemActor, got {packets:?}");
        };
        assert_eq!(add.item.stacksize, 2);
        assert_eq!(add.position.y, 64.125, "wire position is the box centre");
        assert_eq!(add.velocity.y, 0.2);

        sync(&mut world);
        assert!(drain(&mut rx).is_empty(), "nothing changed, nothing sent");

        world.get_mut::<Position>(item).unwrap().0.y = 63.5;
        world.get_mut::<ItemEntityStack>(item).unwrap().0.count = 5;
        sync(&mut world);
        let packets = drain(&mut rx);
        assert!(packets.iter().any(|p| matches!(p,
            McpePacketData::MoveActorAbsolutePacket(m) if m.move_data.position.y == 63.625)));
        assert!(packets.iter().any(|p| matches!(p,
            McpePacketData::ActorEventPacket(e)
                if e.event_id == EnumsActorEvent::UpdateStackSize && e.data == 5)));

        world.get_mut::<ChunkViewers>(chunk).unwrap().remove(player);
        sync(&mut world);
        assert!(matches!(
            drain(&mut rx).as_slice(),
            [McpePacketData::RemoveActorPacket(_)]
        ));

        world.get_mut::<ChunkViewers>(chunk).unwrap().insert(player);
        sync(&mut world);
        assert!(matches!(
            drain(&mut rx).as_slice(),
            [McpePacketData::AddItemActorPacket(_)]
        ));
        world.despawn(item);
        assert!(matches!(
            drain(&mut rx).as_slice(),
            [McpePacketData::RemoveActorPacket(_)]
        ));
    }
}

// =============================================================================
// Arm swings
// =============================================================================

/// Relay a player's arm swing (`Animate` with the swing action) to the
/// players around them and raise the `on-player-swing` plugin event.
///
/// The client sends `Animate(Swing)` for its own swings; the server forwards
/// it to the other players that can see the swinger. Packets for another actor or
/// with a non-swing action are ignored: clients do not drive those.
pub fn relay_player_swing(
    world: &mut World,
    player: Entity,
    packet: &jolyne::valentine::AnimatePacket,
) {
    use jolyne::valentine::types::EnumsAnimatePacketPayloadAction;
    if packet.action != EnumsAnimatePacketPayloadAction::Swing {
        tracing::debug!(action = ?packet.action, "Ignoring non-swing Animate from client");
        return;
    }
    let (Some(runtime_id), Some(position)) = (
        world.get::<RuntimeEntityId>(player).map(|rid| rid.0),
        world.get::<Position>(player).copied(),
    ) else {
        return;
    };
    if packet.target_actor_runtime_id.actor_runtime_id != runtime_id as u64 {
        tracing::debug!(player = ?player, "Ignoring Animate for another actor");
        return;
    }
    if let Some(mut events) = world.get_resource_mut::<crate::ecs::events::EventBuffer>() {
        events.push(crate::ecs::events::ServerEvent::PlayerSwing { entity: player });
    }
    let chunk = (
        (position.0.x.floor() as i32) >> 4,
        (position.0.z.floor() as i32) >> 4,
    );
    let Some(nearby) = world
        .get_resource::<EntityGrid>()
        .map(|grid| grid.get_neighbors(chunk, 1))
    else {
        return;
    };
    let relay = McpePacket::from(jolyne::valentine::AnimatePacket {
        action: EnumsAnimatePacketPayloadAction::Swing,
        target_actor_runtime_id: ActorRuntimeId {
            actor_runtime_id: runtime_id as u64,
        },
        data: packet.data,
        swing_source: packet.swing_source.clone(),
    });
    for viewer in nearby {
        if viewer == player {
            continue;
        }
        if player_views::can_receive_actor(world, player, viewer)
            && let Some(session) = world.get::<PlayerSession>(viewer)
        {
            let _ = session.send(relay.clone());
        }
    }
}

#[cfg(test)]
mod swing_tests {
    use super::*;
    use crate::ecs::events::{EventBuffer, ServerEvent};
    use glam::DVec3;
    use jolyne::valentine::types::EnumsAnimatePacketPayloadAction;
    use jolyne::valentine::{AnimatePacket, McpePacketData};

    fn player(
        world: &mut World,
        runtime_id: i64,
        x: f64,
    ) -> (Entity, tokio::sync::mpsc::Receiver<McpePacket>) {
        let (tx, rx) = tokio::sync::mpsc::channel(8);
        let position = Position(DVec3::new(x, 64.0, 0.0));
        let entity = world
            .spawn((
                RuntimeEntityId(runtime_id),
                position,
                SpatialChunk::from_position(&position),
                PlayerSession::new(runtime_id as u64, "p".into(), None, None, tx),
            ))
            .id();
        (entity, rx)
    }

    fn swing(target: i64, action: EnumsAnimatePacketPayloadAction) -> AnimatePacket {
        AnimatePacket {
            action,
            target_actor_runtime_id: ActorRuntimeId {
                actor_runtime_id: target as u64,
            },
            data: 0.0,
            swing_source: Some("attack".into()),
        }
    }

    #[test]
    fn swings_reach_nearby_players_and_fire_the_plugin_event() {
        let mut world = World::new();
        world.insert_resource(EntityGrid::default());
        world.insert_resource(EventBuffer::default());
        let (swinger, mut own) = player(&mut world, 1, 0.5);
        let (_near, mut near) = player(&mut world, 2, 20.5);
        let (_far, mut far) = player(&mut world, 3, 200.5);

        relay_player_swing(
            &mut world,
            swinger,
            &swing(1, EnumsAnimatePacketPayloadAction::Swing),
        );

        let McpePacketData::AnimatePacket(relayed) = near.try_recv().unwrap().data else {
            panic!("nearby player sees the swing");
        };
        assert_eq!(relayed.target_actor_runtime_id.actor_runtime_id, 1);
        assert_eq!(relayed.action, EnumsAnimatePacketPayloadAction::Swing);
        assert!(own.try_recv().is_err(), "not echoed to the swinger");
        assert!(far.try_recv().is_err(), "out of range");
        assert!(matches!(
            world.resource_mut::<EventBuffer>().drain().as_slice(),
            [ServerEvent::PlayerSwing { entity }] if *entity == swinger
        ));
    }

    #[test]
    fn forged_or_non_swing_animations_are_ignored() {
        let mut world = World::new();
        world.insert_resource(EntityGrid::default());
        world.insert_resource(EventBuffer::default());
        let (swinger, _own) = player(&mut world, 1, 0.5);
        let (_near, mut near) = player(&mut world, 2, 2.5);
        relay_player_swing(
            &mut world,
            swinger,
            &swing(2, EnumsAnimatePacketPayloadAction::Swing),
        );
        relay_player_swing(
            &mut world,
            swinger,
            &swing(1, EnumsAnimatePacketPayloadAction::Criticalhit),
        );
        assert!(near.try_recv().is_err());
        assert!(world.resource_mut::<EventBuffer>().drain().is_empty());
    }
}
