//! Client view of mobs and projectiles: spawn, state replication and
//! movement. Experience orbs move on the wire the same way
//! ([`replicate_movement`], called from `server::broadcast`).
//!
//! Viewers are the players in send range of the mob (`entity::send_range`:
//! within a 17-square chunk circle, whatever the view distance). A new viewer
//! gets one `AddActor` carrying the full metadata, synced properties and
//! attributes; in the tick the mob spawned it carries the position, velocity
//! and rotation the mob spawned with, and the tick's movement follows it.
//! Existing viewers get, per tick and only for what changed: `SetActorData`
//! (coalesced dirty metadata keys plus the property snapshot),
//! `UpdateAttributes`, the equipment slots that changed, `MoveActorDelta`
//! when movement is significant and `SetActorMotion` when velocity changed
//! enough (thresholds in [`constants`]); a new viewer of a mob with equipment
//! also gets its equipment after the `AddActor` (`mob::gear::replicate`). A
//! viewer out of range gets `RemoveActor` one tick later.
//!
//! Movement follows BDS 1.26.51 (`docs/quirks/bds.md`, "Entities / movement
//! replication"). One movement state per mob is "last sent": a
//! `MoveActorDelta` is due when the mob moved or turned enough since it, or
//! its ground contact changed, and carries the axes and rotation bytes that
//! differ from it. Each viewer gets a due update only when the mob's
//! [`BandwidthOptimization`] allows: at least `dropped ticks` since the last
//! one it got, from its distance (eye to the mob's position), with the values
//! in force for whether the mob moved in the previous tick; a change of
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
use crate::entity::mob::components::{BandwidthOptimization, BandwidthValues};
use crate::entity::mob::gear::{MobEquipment, replicate};
use crate::entity::mob::{MobKind, constants};
use crate::entity::send_range::{Candidates, ViewerChanges};
use crate::server::game::types::{ItemRegistryResource, ServerConfigResource};

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
    /// Where the actor was at the previous sync.
    previous: Option<DVec3>,
    /// It moved between the two syncs before this one: the `is_moving` its
    /// bandwidth values are picked by.
    moving: bool,
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
            previous: Some(position),
            ..Self::default()
        }
    }

    /// No motion counts as sent yet, whatever the `AddActor` shows: the
    /// first `SetActorMotion` follows once the velocity is 0.2 from zero (a
    /// BDS 1.26.51 orb summoned at 0.12 upward got its first one only when
    /// falling at 0.225, though 0.31 from the `AddActor` velocity before).
    pub fn motion_unsent(mut self) -> Self {
        self.velocity = DVec3::ZERO;
        self
    }

    /// The state an `AddActor` sent this tick shows (position, velocity,
    /// rotation): in the spawn tick the one the actor spawned with.
    pub fn shown(
        &self,
        position: DVec3,
        velocity: DVec3,
        rotation: Rotation,
    ) -> (DVec3, DVec3, Rotation) {
        self.spawned
            .map_or((position, velocity, rotation), |spawn| {
                (spawn.position, spawn.velocity, spawn.rotation)
            })
    }

    /// This is the sync of the tick the actor spawned in: its `AddActor`
    /// shows the spawn state and its first move follows at once.
    pub fn in_spawn_tick(&self) -> bool {
        self.spawned.is_some()
    }

    /// The spawn tick's sync is done.
    pub fn end_spawn_tick(&mut self) {
        self.spawned = None;
    }

    /// Whether the actor moved in the previous tick (BDS's `is_moving` as
    /// its bandwidth values see it), remembering where it is now for the
    /// next tick.
    fn advance(&mut self, position: DVec3) -> bool {
        let moving = self.moving;
        self.moving = self.previous.is_some_and(|previous| previous != position);
        self.previous = Some(position);
        moving
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

    /// Forget viewers that no longer see the actor (a new viewer, or a later
    /// entity with a reused id, starts afresh).
    pub fn forget<'a>(&mut self, lost: impl IntoIterator<Item = &'a Entity>) {
        for lost in lost {
            self.deliveries.retain(|(viewer, _)| viewer != lost);
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

/// Degrees cut to whole 1/256 turns, truncated toward zero, as an
/// `AddActor` carries every rotation (BDS 1.26.51: a dispensed arrow at
/// pitch -5.768 and yaw 89.885 is sent as -5.625 and 88.59375, a cow
/// spawned at yaw 33.3 with yaw, head and body yaw 32.34375).
pub fn quantized_degrees(degrees: f32) -> f32 {
    (degrees * (256.0 / 360.0)) as i32 as f32 * (360.0 / 256.0)
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

/// The `AddActor` a new viewer receives, its rotation cut to whole rotation
/// bytes. A living mob's head and body yaw are its own; any other actor (a
/// projectile) has head and body yaw 0, as BDS 1.26.51 sends it.
#[allow(clippy::too_many_arguments)]
pub fn add_actor_packet(
    identifier: &str,
    runtime_id: i64,
    position: DVec3,
    velocity: DVec3,
    rotation: &Rotation,
    living: bool,
    metadata: &EntityMetadata,
    properties: &ActorProperties,
    attributes: &ActorAttributes,
) -> AddActorPacket {
    let (head_yaw, body_yaw) = if living {
        (rotation.head_yaw, rotation.yaw)
    } else {
        (0.0, 0.0)
    };
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
            x: quantized_degrees(rotation.pitch),
            y: quantized_degrees(rotation.yaw),
        },
        y_head_rotation: quantized_degrees(head_yaw),
        y_body_rotation: quantized_degrees(body_yaw),
        attributes_list: attributes.snapshot().into_entries(),
        actor_data: metadata.snapshot(),
        synched_properties: properties_snapshot(properties),
        actor_links: Vec::new(),
    }
}

/// One actor's movement in this tick, as its viewers are told about it.
#[derive(Debug, Clone, Copy)]
pub struct Movement {
    pub runtime_id: u64,
    /// The position on the wire (the actor's position plus any network
    /// offset).
    pub position: DVec3,
    pub rotation: [u8; 3],
    pub on_ground: bool,
    pub velocity: DVec3,
}

impl Movement {
    pub fn new(
        runtime_id: i64,
        position: DVec3,
        rotation: &Rotation,
        on_ground: bool,
        velocity: DVec3,
    ) -> Self {
        Self {
            runtime_id: runtime_id as u64,
            position,
            rotation: rotation_bytes(rotation),
            on_ground,
            velocity,
        }
    }
}

/// Send `movers` this tick's `MoveActorDelta` and `SetActorMotion` of an
/// actor (see the module docs). `eye` is a viewer's eye position, `send`
/// delivers a packet to a viewer. `bandwidth` is the actor's
/// `conditional_bandwidth_optimization` (`None`: every tick).
pub fn replicate_movement(
    net: &mut MobNetState,
    movement: &Movement,
    bandwidth: Option<&BandwidthOptimization>,
    movers: &[Entity],
    now: u64,
    mut eye: impl FnMut(Entity) -> Option<DVec3>,
    mut send: impl FnMut(Entity, &McpePacket),
) {
    let moving = net.advance(movement.position);
    let values: Option<BandwidthValues> = bandwidth.map(|bandwidth| bandwidth.values(moving));
    let rid = ActorRuntimeId {
        actor_runtime_id: movement.runtime_id,
    };
    let position = movement.position;
    let bytes = movement.rotation;
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
    let moved_sq = position.distance_squared(net.position);
    let settling = moved_sq > 1.0e-6 && movement.velocity.length_squared() < 1.0e-6;
    let ground_changed = movement.on_ground != net.on_ground;
    if turned_degrees > constants::NET_ROTATION_THRESHOLD
        || moved_sq > constants::NET_POSITION_THRESHOLD_SQ
        || settling
    {
        let axis = |now: f64, then: f64| (now != then).then_some(now as f32);
        let turned = |i: usize| (bytes[i] != net.rotation[i]).then_some(bytes[i] as i8);
        let mut move_data = MoveActorDeltaData {
            actor_runtime_id: rid.clone(),
            new_position_x: axis(position.x, net.position.x),
            new_position_y: axis(position.y, net.position.y),
            new_position_z: axis(position.z, net.position.z),
            rotation_x: turned(0),
            rotation_y: turned(1),
            rotation_y_head: turned(2),
            is_on_ground: movement.on_ground,
            ..Default::default()
        };
        let mut delivered = false;
        for viewer in movers {
            // A player's position is its eye.
            let dropped = match (values, eye(*viewer)) {
                (Some(values), Some(eye)) => values.dropped_ticks(eye.distance(position)),
                _ => 0,
            };
            let waiting = net
                .last_delivery(*viewer)
                .is_some_and(|last| now.saturating_sub(last) < dropped);
            if waiting && !ground_changed {
                continue;
            }
            move_data.ticks = if ground_changed { 0 } else { dropped };
            send(
                *viewer,
                &McpePacket::from(MoveActorDeltaPacket {
                    move_data: move_data.clone(),
                }),
            );
            net.record_delivery(*viewer, now);
            delivered = true;
        }
        if delivered {
            net.position = position;
            net.rotation = bytes;
            net.on_ground = movement.on_ground;
        }
    }
    let stopped = movement.velocity == DVec3::ZERO && net.velocity != DVec3::ZERO;
    if (movement.velocity.distance_squared(net.velocity) > constants::NET_VELOCITY_THRESHOLD_SQ
        || stopped)
        && !movers.is_empty()
    {
        let motion = McpePacket::from(SetActorMotionPacket {
            target_runtime_id: rid,
            motion: wire(movement.velocity),
            tick: PlayerInputTick { inputtick: 0 },
        });
        for viewer in movers {
            send(*viewer, &motion);
        }
        net.velocity = movement.velocity;
    }
}

/// System: keep every mob spawned on exactly the clients in its send range
/// and replicate what changed this tick.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub fn sync_mob_viewers(
    sessions: Query<(Entity, &PlayerSession, Option<&Position>)>,
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
    let candidates = Candidates::new(
        sessions
            .iter()
            .filter_map(|(entity, _, position)| Some((entity, position?.0))),
    );
    let eye = |viewer: Entity| {
        sessions
            .get(viewer)
            .ok()
            .and_then(|(_, _, position)| position.map(|position| position.0))
    };
    let send = |viewer: Entity, packet: &McpePacket| {
        if let Ok((_, session, _)) = sessions.get(viewer) {
            let _ = session.send(packet.clone());
        }
    };
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
        let desired = candidates.in_range_of(position.0, None);
        let rid = ActorRuntimeId {
            actor_runtime_id: runtime_id.0 as u64,
        };

        let changes = ViewerChanges::update(&mut viewers, &desired, &candidates);
        if !changes.gone.is_empty() {
            let remove = McpePacket::from(RemoveActorPacket {
                target_actor_id: ActorUniqueId {
                    actor_unique_id: runtime_id.0,
                },
            });
            for viewer in &changes.gone {
                send(*viewer, &remove);
            }
        }
        net.forget(changes.lost());

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
            for packet in &updates {
                send(*viewer, packet);
            }
        }

        // New viewers get the full current state, or in the spawn tick the
        // state the mob spawned with followed by this tick's movement.
        let spawn_tick = net.in_spawn_tick();
        let mut movers = viewers.viewers.clone();
        if !changes.fresh.is_empty() {
            let (shown_position, shown_velocity, shown_rotation) =
                net.shown(position.0, velocity.0, *rotation);
            let spawn = McpePacket::from(add_actor_packet(
                &kind.0.identifier,
                runtime_id.0,
                shown_position,
                shown_velocity,
                &shown_rotation,
                !kind.0.projectile,
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
            for viewer in changes.fresh {
                if let Ok((_, session, _)) = sessions.get(viewer)
                    && session.send(spawn.clone())
                {
                    viewers.viewers.push(viewer);
                    for packet in equipment.iter().flatten() {
                        let _ = session.send(packet.clone());
                    }
                    if spawn_tick {
                        movers.push(viewer);
                    }
                }
            }
        }
        net.end_spawn_tick();

        replicate_movement(
            &mut net,
            &Movement::new(runtime_id.0, position.0, rotation, on_ground.0, velocity.0),
            bandwidth,
            &movers,
            now,
            eye,
            send,
        );
    }
}
