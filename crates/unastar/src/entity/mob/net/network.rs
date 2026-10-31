//! Client view of mobs: spawn, state replication and movement.
//!
//! Viewers are the players whose loaded chunks contain the mob (the same
//! rule as item actors). A new viewer gets one `AddActor` carrying the full
//! metadata, synced properties and attributes. Existing viewers get, per
//! tick and only for what changed: `SetActorData` (coalesced dirty metadata
//! keys plus the property snapshot), `UpdateAttributes`, `MoveActorDelta`
//! when movement is significant, `SetActorMotion` when velocity changed
//! enough (thresholds in [`constants`]) and the equipment slots that
//! changed; a new viewer of a mob with equipment also gets its equipment
//! after the `AddActor` (`mob::gear::replicate`).

use bevy_ecs::prelude::*;
use glam::DVec3;
use jolyne::valentine::types::{
    ActorRuntimeId, ActorUniqueId, AttributeData, MoveActorDeltaData, PlayerInputTick,
    PropertySyncData, SynchedActorDataCopyableDataList, Vec2, Vec3,
};
use jolyne::valentine::{
    AddActorPacket, McpePacket, MoveActorDeltaPacket, RemoveActorPacket, SetActorDataPacket,
    SetActorMotionPacket, UpdateAttributesPacket,
};

use crate::entity::components::{
    ActorViewers, OnGround, PlayerSession, Position, Rotation, RuntimeId, Velocity,
};
use crate::entity::definition::attributes::ActorAttributes;
use crate::entity::definition::metadata::EntityMetadata;
use crate::entity::definition::properties::ActorProperties;
use crate::entity::mob::gear::{MobEquipment, replicate};
use crate::entity::mob::{MobKind, constants};
use crate::server::game::types::{ItemRegistryResource, ServerConfigResource};
use crate::world::ChunkManager;
use crate::world::ecs::ChunkViewers;

/// Last replicated state, per mob (shared by all viewers: every viewer
/// receives every update while it is a viewer).
#[derive(Component, Debug, Default)]
pub struct MobNetState {
    pub position: DVec3,
    /// Rotation bytes: pitch, yaw, head yaw.
    pub rotation: [u8; 3],
    pub on_ground: bool,
    pub velocity: DVec3,
    pub metadata_revision: u64,
    pub properties_revision: u64,
    pub attributes_revision: u64,
}

/// Degrees to the protocol's 1/256-turn rotation byte.
pub fn rotation_byte(degrees: f32) -> u8 {
    (degrees.rem_euclid(360.0) * 256.0 / 360.0).round() as u32 as u8
}

fn wire(v: DVec3) -> Vec3 {
    Vec3 {
        x: v.x as f32,
        y: v.y as f32,
        z: v.z as f32,
    }
}

fn properties_snapshot(properties: &ActorProperties) -> PropertySyncData {
    let mut data = PropertySyncData::default();
    properties.copy_synced(&mut data);
    data
}

/// The `AddActor` a new viewer receives.
#[allow(clippy::too_many_arguments)]
pub fn add_actor_packet(
    identifier: &str,
    runtime_id: i64,
    position: DVec3,
    velocity: DVec3,
    rotation: &Rotation,
    metadata: &EntityMetadata,
    properties: &ActorProperties,
    attributes: &ActorAttributes,
) -> AddActorPacket {
    AddActorPacket {
        target_actor_id: ActorUniqueId {
            actor_unique_id: runtime_id,
        },
        target_runtime_id: ActorRuntimeId {
            actor_runtime_id: runtime_id as u64,
        },
        actor_type: identifier.into(),
        position: wire(position),
        velocity: wire(velocity),
        rotation: Vec2 {
            x: rotation.pitch,
            y: rotation.yaw,
        },
        y_head_rotation: rotation.head_yaw,
        y_body_rotation: rotation.yaw,
        attributes_list: attributes.snapshot().into_entries(),
        actor_data: metadata.snapshot(),
        synched_properties: properties_snapshot(properties),
        actor_links: Vec::new(),
    }
}

/// System: keep every mob spawned on exactly the clients that view its
/// chunk and replicate what changed this tick.
#[allow(clippy::type_complexity)]
pub fn sync_mob_viewers(
    chunk_manager: Res<ChunkManager>,
    chunk_viewers: Query<&ChunkViewers>,
    sessions: Query<&PlayerSession>,
    items: Option<Res<ItemRegistryResource>>,
    config: Option<Res<ServerConfigResource>>,
    mut mobs: Query<(
        Entity,
        &MobKind,
        &RuntimeId,
        &Position,
        &Rotation,
        &Velocity,
        &OnGround,
        &mut EntityMetadata,
        &ActorProperties,
        &mut ActorAttributes,
        &mut ActorViewers,
        &mut MobNetState,
        Option<&mut MobEquipment>,
        Option<&crate::entity::components::ActorAudience>,
    )>,
) {
    let block_ids = config
        .as_ref()
        .map_or_else(Default::default, |config| config.0.block_network_ids);
    for (
        actor,
        kind,
        runtime_id,
        position,
        rotation,
        velocity,
        on_ground,
        mut metadata,
        properties,
        mut attributes,
        mut viewers,
        mut net,
        mut gear,
        audience,
    ) in &mut mobs
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
        let rid = ActorRuntimeId {
            actor_runtime_id: runtime_id.0 as u64,
        };

        let gone: Vec<Entity> = viewers
            .viewers
            .iter()
            .copied()
            .filter(|v| !desired.contains(v))
            .collect();
        if !gone.is_empty() {
            let remove = McpePacket::from(RemoveActorPacket {
                target_actor_id: ActorUniqueId {
                    actor_unique_id: runtime_id.0,
                },
            });
            viewers.viewers.retain(|viewer| {
                !gone.contains(viewer)
                    || sessions
                        .get(*viewer)
                        .is_ok_and(|session| !session.send(remove.clone()))
            });
        }

        // Updates for existing viewers.
        let mut updates: Vec<McpePacket> = Vec::new();
        if metadata.is_dirty() || properties.sync_revision() != net.properties_revision {
            let mut changes = SynchedActorDataCopyableDataList::default();
            metadata.copy_changes(&mut changes);
            updates.push(McpePacket::from(SetActorDataPacket {
                target_runtime_id: rid.clone(),
                actor_data: changes,
                synched_properties: properties_snapshot(properties),
                tick: PlayerInputTick { inputtick: 0 },
            }));
            metadata.mark_clean();
            net.properties_revision = properties.sync_revision();
        }
        if attributes.is_dirty() {
            let snapshot = attributes.snapshot();
            updates.push(McpePacket::from(UpdateAttributesPacket {
                target_runtime_id: rid.clone(),
                attribute_list: snapshot
                    .entries()
                    .iter()
                    .map(|row| AttributeData {
                        min_value: row.min_value,
                        max_value: row.max_value,
                        current_value: row.current_value,
                        default_min_value: row.min_value,
                        default_max_value: row.max_value,
                        default_value: row.current_value,
                        name: row.attribute_name.clone(),
                        modifiers: Vec::new(),
                    })
                    .collect(),
                tick: PlayerInputTick { inputtick: 0 },
            }));
            attributes.mark_clean();
        }
        let bytes = [
            rotation_byte(rotation.pitch),
            rotation_byte(rotation.yaw),
            rotation_byte(rotation.head_yaw),
        ];
        // Native significance rule: summed quantised rotation change above
        // 4 degrees or squared movement above 1e-4; plus one settling update
        // when a tiny residual move ends at rest.
        let turned_degrees: f32 = bytes
            .iter()
            .zip(net.rotation.iter())
            .map(|(now, then)| f32::from(now.wrapping_sub(*then) as i8).abs() * 360.0 / 256.0)
            .sum();
        let moved_sq = position.0.distance_squared(net.position);
        let settling = moved_sq > 1.0e-6 && velocity.0.length_squared() < 1.0e-6;
        if turned_degrees > constants::NET_ROTATION_THRESHOLD
            || moved_sq > constants::NET_POSITION_THRESHOLD_SQ
            || settling
            || on_ground.0 != net.on_ground
        {
            let axis = |now: f64, then: f64| (now != then).then_some(now as f32);
            let turned = |i: usize| (bytes[i] != net.rotation[i]).then_some(bytes[i] as i8);
            updates.push(McpePacket::from(MoveActorDeltaPacket {
                move_data: MoveActorDeltaData {
                    actor_runtime_id: rid.clone(),
                    new_position_x: axis(position.0.x, net.position.x),
                    new_position_y: axis(position.0.y, net.position.y),
                    new_position_z: axis(position.0.z, net.position.z),
                    rotation_x: turned(0),
                    rotation_y: turned(1),
                    rotation_y_head: turned(2),
                    is_on_ground: on_ground.0,
                    ..Default::default()
                },
            }));
            net.position = position.0;
            net.rotation = bytes;
            net.on_ground = on_ground.0;
        }
        let stopped = velocity.0 == DVec3::ZERO && net.velocity != DVec3::ZERO;
        if velocity.0.distance_squared(net.velocity) > constants::NET_VELOCITY_THRESHOLD_SQ
            || stopped
        {
            updates.push(McpePacket::from(SetActorMotionPacket {
                target_runtime_id: rid.clone(),
                motion: wire(velocity.0),
                tick: PlayerInputTick { inputtick: 0 },
            }));
            net.velocity = velocity.0;
        }
        if let (Some(gear), Some(items)) = (gear.as_mut(), items.as_ref())
            && gear.is_dirty()
        {
            replicate::changed_packets(runtime_id.0, gear, &items.0, block_ids, &mut updates);
        }
        for viewer in viewers
            .viewers
            .iter()
            .filter(|viewer| desired.contains(viewer))
        {
            if let Ok(session) = sessions.get(*viewer) {
                for packet in &updates {
                    let _ = session.send(packet.clone());
                }
            }
        }

        // New viewers get the full current state.
        let fresh: Vec<Entity> = desired
            .iter()
            .copied()
            .filter(|v| !viewers.viewers.contains(v))
            .collect();
        if !fresh.is_empty() {
            let spawn = McpePacket::from(add_actor_packet(
                &kind.0.identifier,
                runtime_id.0,
                position.0,
                velocity.0,
                rotation,
                &metadata,
                properties,
                &attributes,
            ));
            let equipment = gear
                .as_deref()
                .filter(|gear| gear.count() > 0)
                .zip(items.as_ref())
                .map(|(gear, items)| {
                    replicate::inventory_packets(runtime_id.0, gear, &items.0, block_ids)
                });
            for viewer in fresh {
                if let Ok(session) = sessions.get(viewer)
                    && session.send(spawn.clone())
                {
                    viewers.viewers.push(viewer);
                    for packet in equipment.iter().flatten() {
                        let _ = session.send(packet.clone());
                    }
                }
            }
        }
    }
}
