//! Client view of mobs: spawn, state replication and movement.
//!
//! Viewers are the players whose loaded chunks contain the mob (the same
//! rule as item actors). A new viewer gets one `AddActor` carrying the full
//! metadata, synced properties and attributes; in the tick the mob spawned
//! it carries the position, velocity and rotation the mob spawned with, and
//! the tick's movement follows it. Existing viewers get, per tick and only
//! for what changed: `SetActorData` (coalesced dirty metadata keys plus the
//! property snapshot), `UpdateAttributes`, the equipment slots that changed,
//! `MoveActorDelta` when movement is significant and `SetActorMotion` when
//! velocity changed enough (thresholds in [`constants`]); a new viewer of a
//! mob with equipment also gets its equipment after the `AddActor`
//! (`mob::gear::replicate`).
//!
//! Movement follows BDS 1.26.51 (`docs/quirks/bds.md`, "Entities / movement
//! replication"). One movement state per mob is "last sent": a
//! `MoveActorDelta` is due when the mob moved or turned enough since it, or
//! its ground contact changed, and carries the axes and rotation bytes that
//! differ from it. Each viewer gets a due update only when the mob's
//! [`BandwidthOptimization`] allows: at least `dropped ticks` since the last
//! one it got, from its distance (eye to the mob's position); a change of
//! ground contact, and a viewer's first update, go at once. The delta's
//! `ticks` is those dropped ticks, 0 on a ground change. The last-sent state
//! moves on only when at least one viewer got the update, so a far viewer
//! keeps a stale axis that changed only between its updates while a nearer
//! viewer got it. Motion packets go to every viewer.

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

use crate::ecs::resources::TickCounter;
use crate::entity::components::{
    ActorViewers, OnGround, PlayerSession, Position, Rotation, RuntimeId, Velocity,
};
use crate::entity::definition::attributes::ActorAttributes;
use crate::entity::definition::metadata::EntityMetadata;
use crate::entity::definition::properties::ActorProperties;
use crate::entity::mob::components::BandwidthOptimization;
use crate::entity::mob::gear::{MobEquipment, replicate};
use crate::entity::mob::{MobKind, constants};
use crate::server::game::types::{ItemRegistryResource, ServerConfigResource};
use crate::world::ChunkManager;
use crate::world::ecs::ChunkViewers;

/// The movement state last sent to at least one viewer, shared by all of
/// them, and when each viewer last got a position update.
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
    /// The tick each viewer last got a `MoveActorDelta`.
    deliveries: Vec<(Entity, u64)>,
    /// The state the mob spawned with, until the sync of its spawn tick.
    spawned: Option<SpawnState>,
}

/// What a mob's `AddActor` carries in the tick it spawned.
#[derive(Debug, Clone, Copy)]
struct SpawnState {
    position: DVec3,
    velocity: DVec3,
    rotation: Rotation,
}

impl MobNetState {
    /// The state of a mob that has just spawned: its first viewers see it
    /// as it was before this tick's move, and its first updates are
    /// measured from there.
    pub fn spawned(position: DVec3, velocity: DVec3, rotation: Rotation, on_ground: bool) -> Self {
        Self {
            position,
            rotation: rotation_bytes(&rotation),
            on_ground,
            velocity,
            spawned: Some(SpawnState {
                position,
                velocity,
                rotation,
            }),
            ..Self::default()
        }
    }

    fn last_delivery(&self, viewer: Entity) -> Option<u64> {
        self.deliveries
            .iter()
            .find_map(|(entity, tick)| (*entity == viewer).then_some(*tick))
    }

    fn record_delivery(&mut self, viewer: Entity, tick: u64) {
        match self
            .deliveries
            .iter_mut()
            .find(|(entity, _)| *entity == viewer)
        {
            Some(entry) => entry.1 = tick,
            None => self.deliveries.push((viewer, tick)),
        }
    }
}

/// Snapshot `mob`'s current position, velocity and rotation as its spawn
/// state (see [`MobNetState::spawned`]), with `reported_on_ground` as its
/// ground contact, or its own when `None`.
pub fn record_spawn_state(world: &mut World, mob: Entity, reported_on_ground: Option<bool>) {
    let Some(state) = (|| {
        Some(MobNetState::spawned(
            world.get::<Position>(mob)?.0,
            world.get::<Velocity>(mob)?.0,
            *world.get::<Rotation>(mob)?,
            reported_on_ground.or_else(|| world.get::<OnGround>(mob).map(|ground| ground.0))?,
        ))
    })() else {
        return;
    };
    world.entity_mut(mob).insert(state);
}

/// Degrees to the protocol's 1/256-turn rotation byte: the angle in
/// 1/256 turns, truncated toward zero, wrapped into a byte (BDS 1.26.51:
/// pitch -53.136 is 219 and yaw 29.491 is 20 in its `MoveActorDelta`).
pub fn rotation_byte(degrees: f32) -> u8 {
    (degrees * (256.0 / 360.0)) as i32 as u8
}

fn rotation_bytes(rotation: &Rotation) -> [u8; 3] {
    [
        rotation_byte(rotation.pitch),
        rotation_byte(rotation.yaw),
        rotation_byte(rotation.head_yaw),
    ]
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
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub fn sync_mob_viewers(
    chunk_manager: Res<ChunkManager>,
    chunk_viewers: Query<&ChunkViewers>,
    sessions: Query<(&PlayerSession, Option<&Position>)>,
    tick: Res<TickCounter>,
    items: Option<Res<ItemRegistryResource>>,
    config: Option<Res<ServerConfigResource>>,
    mut mobs: Query<(
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
        Option<&BandwidthOptimization>,
    )>,
) {
    let block_ids = config
        .as_ref()
        .map_or_else(Default::default, |config| config.0.block_network_ids);
    let now = tick.current;
    for (
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
        bandwidth,
    ) in &mut mobs
    {
        let (cx, cz) = crate::world::ecs::world_to_chunk_coords(
            position.0.x.floor() as i32,
            position.0.z.floor() as i32,
        );
        let desired: Vec<Entity> = chunk_manager
            .get_by_coords(cx, cz)
            .and_then(|chunk| chunk_viewers.get(chunk).ok())
            .map(|v| v.iter().filter(|e| sessions.contains(*e)).collect())
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
            for viewer in &gone {
                if let Ok((session, _)) = sessions.get(*viewer) {
                    let _ = session.send(remove.clone());
                }
            }
            viewers.viewers.retain(|v| !gone.contains(v));
            net.deliveries.retain(|(v, _)| !gone.contains(v));
        }

        // State updates for existing viewers.
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
        if let (Some(gear), Some(items)) = (gear.as_mut(), items.as_ref())
            && gear.is_dirty()
        {
            replicate::changed_packets(runtime_id.0, gear, &items.0, block_ids, &mut updates);
        }
        for viewer in &viewers.viewers {
            if let Ok((session, _)) = sessions.get(*viewer) {
                for packet in &updates {
                    let _ = session.send(packet.clone());
                }
            }
        }

        // New viewers get the full current state, or in the spawn tick the
        // state the mob spawned with followed by this tick's movement.
        let spawned = net.spawned.take();
        let mut movers = viewers.viewers.clone();
        let fresh: Vec<Entity> = desired
            .iter()
            .copied()
            .filter(|v| !viewers.viewers.contains(v))
            .collect();
        if !fresh.is_empty() {
            let (shown_position, shown_velocity, shown_rotation) = spawned
                .map_or((position.0, velocity.0, *rotation), |spawn| {
                    (spawn.position, spawn.velocity, spawn.rotation)
                });
            let spawn = McpePacket::from(add_actor_packet(
                &kind.0.identifier,
                runtime_id.0,
                shown_position,
                shown_velocity,
                &shown_rotation,
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
                if let Ok((session, _)) = sessions.get(viewer)
                    && session.send(spawn.clone())
                {
                    viewers.viewers.push(viewer);
                    for packet in equipment.iter().flatten() {
                        let _ = session.send(packet.clone());
                    }
                    if spawned.is_some() {
                        movers.push(viewer);
                    }
                }
            }
        }

        // Movement, measured from the state last sent to any viewer.
        let bytes = rotation_bytes(rotation);
        // Native significance rule: summed quantised rotation change above
        // 4 degrees or squared movement above 1e-4, plus one settling update
        // when a tiny residual move ends at rest. A change of ground contact
        // alone is not enough (a mob spawned in mid-air leaves the ground
        // with its 1e-5 first move and BDS sends nothing); it only lets a
        // due update past every viewer's cadence.
        let turned_degrees: f32 = bytes
            .iter()
            .zip(net.rotation.iter())
            .map(|(now, then)| f32::from(now.wrapping_sub(*then) as i8).abs() * 360.0 / 256.0)
            .sum();
        let moved_sq = position.0.distance_squared(net.position);
        let settling = moved_sq > 1.0e-6 && velocity.0.length_squared() < 1.0e-6;
        let ground_changed = on_ground.0 != net.on_ground;
        if turned_degrees > constants::NET_ROTATION_THRESHOLD
            || moved_sq > constants::NET_POSITION_THRESHOLD_SQ
            || settling
        {
            let axis = |now: f64, then: f64| (now != then).then_some(now as f32);
            let turned = |i: usize| (bytes[i] != net.rotation[i]).then_some(bytes[i] as i8);
            let mut move_data = MoveActorDeltaData {
                actor_runtime_id: rid.clone(),
                new_position_x: axis(position.0.x, net.position.x),
                new_position_y: axis(position.0.y, net.position.y),
                new_position_z: axis(position.0.z, net.position.z),
                rotation_x: turned(0),
                rotation_y: turned(1),
                rotation_y_head: turned(2),
                is_on_ground: on_ground.0,
                ..Default::default()
            };
            let mut delivered = false;
            for viewer in &movers {
                let Ok((session, eye)) = sessions.get(*viewer) else {
                    continue;
                };
                // A player's position is its eye.
                let dropped = bandwidth.zip(eye).map_or(0, |(bandwidth, eye)| {
                    bandwidth.0.dropped_ticks(eye.0.distance(position.0))
                });
                let waiting = net
                    .last_delivery(*viewer)
                    .is_some_and(|last| now.saturating_sub(last) < dropped);
                if waiting && !ground_changed {
                    continue;
                }
                move_data.ticks = if ground_changed { 0 } else { dropped };
                let _ = session.send(McpePacket::from(MoveActorDeltaPacket {
                    move_data: move_data.clone(),
                }));
                net.record_delivery(*viewer, now);
                delivered = true;
            }
            if delivered {
                net.position = position.0;
                net.rotation = bytes;
                net.on_ground = on_ground.0;
            }
        }
        let stopped = velocity.0 == DVec3::ZERO && net.velocity != DVec3::ZERO;
        if (velocity.0.distance_squared(net.velocity) > constants::NET_VELOCITY_THRESHOLD_SQ
            || stopped)
            && !movers.is_empty()
        {
            let motion = McpePacket::from(SetActorMotionPacket {
                target_runtime_id: rid.clone(),
                motion: wire(velocity.0),
                tick: PlayerInputTick { inputtick: 0 },
            });
            for viewer in &movers {
                if let Ok((session, _)) = sessions.get(*viewer) {
                    let _ = session.send(motion.clone());
                }
            }
            net.velocity = velocity.0;
        }
    }
}
