//! Current player representation and reliable per-viewer reconciliation.

use super::{build_add_player_packet, build_move_player_packet, build_remove_actor_packet};
use crate::entity::components::*;
use crate::entity::definition::flags::ActorFlags;
use crate::entity::definition::metadata::EntityMetadata;
use crate::registry::block::BlockNetworkIds;
use crate::server::game::types::{ItemRegistryResource, ServerConfigResource};
use crate::world::ecs::{PlayerDespawnedEvent, PlayerSpawnedEvent};
use bevy_ecs::message::MessageCursor;
use bevy_ecs::prelude::*;
use jolyne::valentine::types::{
    ActorRuntimeId, CerealizerNetworkItemStackDescriptorSerializedData as WireItem, DataItemEntry,
    DataItemEntryPayload, DataItemInt64Payload, EnumsDataItemType, PropertySyncData,
    SynchedActorDataCopyableDataList,
};
use jolyne::valentine::{
    McpePacket, MobArmorEquipmentPacket, MobEquipmentPacket, SetActorDataPacket,
};
use jolyne::valentine::{actor_data_i_ds as ids, actor_flags};
use std::collections::{HashMap, HashSet};

/// Per-client transport identity, so a replacement connection receives a fresh view.
#[derive(Clone)]
struct Connection {
    id: u64,
    sender: tokio::sync::mpsc::Sender<McpePacket>,
}
impl Connection {
    /// Retain only the transport facts needed to invalidate a client's old cache.
    fn of(session: &PlayerSession) -> Self {
        Self {
            id: session.session_id,
            sender: session.outbound_tx.clone(),
        }
    }
    /// Compare transport identity without depending on a reused session number.
    fn matches(&self, session: &PlayerSession) -> bool {
        self.id == session.session_id && self.sender.same_channel(&session.outbound_tx)
    }
}

/// Last equipment and actor state successfully queued for a particular viewer.
struct KnownPlayer {
    connection: Connection,
    runtime_id: i64,
    uuid: uuid::Uuid,
    name: String,
    mode: GameMode,
    last: LastBroadcastPosition,
    metadata: Vec<DataItemEntry>,
    hands: [Option<WireItem>; 2],
    armour: Option<[WireItem; 4]>,
    selected: u8,
}

/// Spawn readiness and client-known player state, keyed by full ECS identities.
#[derive(Resource, Default)]
struct PlayerViewCache {
    spawned: MessageCursor<PlayerSpawnedEvent>,
    despawned: MessageCursor<PlayerDespawnedEvent>,
    ready: HashSet<Entity>,
    known: HashMap<(Entity, Entity), KnownPlayer>,
}

/// One actor's current immutable representation, captured once for every viewer.
struct PlayerView {
    actor: Entity,
    runtime_id: i64,
    uuid: uuid::Uuid,
    name: String,
    position: Position,
    rotation: Rotation,
    mode: GameMode,
    tick: jolyne::valentine::types::PlayerInputTick,
    on_ground: bool,
    audience: ActorAudience,
    metadata: Vec<DataItemEntry>,
    equipment: [WireItem; 6],
    selected: u8,
}

/// Apply the actor's explicit audience without requiring a spawned client view.
pub fn audience_allows(world: &World, actor: Entity, viewer: Entity) -> bool {
    if crate::server::game::player_events::initial_state_pending(world,actor) || crate::server::game::player_events::initial_state_pending(world,viewer) {return false;}
    world
        .get::<ActorAudience>(actor)
        .is_none_or(|audience| audience.allows(actor, viewer))
}

/// Whether a viewer may receive representation or transient events from an actor.
/// Actor self always remains eligible. Player events additionally require a view
/// already queued for that same client transport once reconciliation is installed.
pub fn can_receive_actor(world: &World, actor: Entity, viewer: Entity) -> bool {
    if !audience_allows(world, actor, viewer) {return false;}
    if actor == viewer {
        return true;
    }
    if !audience_allows(world, actor, viewer) {
        return false;
    }
    if world.get::<Player>(actor).is_none() {
        return true;
    }
    let Some(cache) = world.get_resource::<PlayerViewCache>() else {
        return true;
    };
    let Some(known) = cache.known.get(&(viewer, actor)) else {
        return false;
    };
    world
        .get::<PlayerSession>(viewer)
        .is_some_and(|session| known.connection.matches(session))
        && world
            .get::<RuntimeEntityId>(actor)
            .is_some_and(|id| id.0 == known.runtime_id)
        && world
            .get::<PlayerUuid>(actor)
            .is_some_and(|uuid| uuid.0 == known.uuid)
}

/// Current player flags shared by initial snapshots and using-item updates.
pub(crate) fn player_flags(world: &World, player: Entity, using: bool) -> i64 {
    let mut flags = world
        .get::<ActorFlags>(player)
        .map_or(0, |flags| flags.snapshot().words()[0]);
    flags |= (1u64 << actor_flags::CANCLIMB)
        | (1u64 << actor_flags::BREATHING)
        | (1u64 << actor_flags::HAS_COLLISION)
        | (1u64 << actor_flags::HAS_GRAVITY);
    if let Some(state) = world.get::<PlayerState>(player) {
        for (on, flag) in [
            (state.sneaking, actor_flags::SNEAKING),
            (state.sprinting, actor_flags::SPRINTING),
            (state.swimming, actor_flags::SWIMMING),
            (state.gliding, actor_flags::GLIDING),
        ] {
            if on {
                flags |= 1u64 << flag;
            } else {
                flags &= !(1u64 << flag);
            }
        }
    }
    if using {
        flags |= 1u64 << actor_flags::USINGITEM;
    } else {
        flags &= !(1u64 << actor_flags::USINGITEM);
    }
    if world.get::<Effects>(player).is_some_and(|effects| {
        effects
            .get(EffectType::INVISIBILITY)
            .is_some_and(|effect| !effect.is_expired())
    }) {
        flags |= 1u64 << actor_flags::INVISIBLE;
    }
    if world
        .get::<crate::entity::player_life::environment::Burning>(player)
        .is_some()
        || world
            .get::<OnFire>(player)
            .is_some_and(|fire| fire.is_on_fire())
    {
        flags |= 1u64 << actor_flags::ONFIRE;
    }
    flags as i64
}

/// Build current metadata without depending on a previous movement/effect packet.
fn metadata(world: &World, actor: Entity) -> Vec<DataItemEntry> {
    let mut data = world
        .get::<EntityMetadata>(actor)
        .map_or_else(Vec::new, |metadata| metadata.snapshot().data);
    let mut put = |id, value| {
        let entry = DataItemEntry {
            id,
            payload: DataItemEntryPayload::DataItemInt64Payload(DataItemInt64Payload {
                type_: EnumsDataItemType::Int64,
                value,
            }),
        };
        if let Some(existing) = data.iter_mut().find(|entry| entry.id == id) {
            *existing = entry;
        } else {
            data.push(entry);
        }
    };
    put(
        ids::RESERVED_0,
        player_flags(
            world,
            actor,
            world
                .get::<crate::server::game::item_use::ItemUsing>(actor)
                .is_some(),
        ),
    );
    let mut visible = world
        .get::<Effects>(actor)
        .map_or_else(Vec::new, |effects| {
            effects
                .active
                .iter()
                .filter(|(id, effect)| id.0 < 64 && effect.show_particles && !effect.is_expired())
                .map(|(id, effect)| (id.0, effect.ambient))
                .collect::<Vec<_>>()
        });
    visible.sort_unstable();
    let packed = visible
        .into_iter()
        .take(9)
        .fold(0u64, |packed, (id, ambient)| {
            (packed << 7) | (u64::from(id) << 1) | u64::from(ambient)
        });
    put(ids::VISIBLE_MOB_EFFECTS, packed as i64);
    data.sort_by_key(|entry| entry.id);
    data
}

/// Resolve current inventory or projected slots into network descriptors.
fn equipment(world: &World, actor: Entity) -> [WireItem; 6] {
    let projection = world.get::<PlayerEquipmentProjection>(actor);
    let held = world
        .get::<HeldSlot>(actor)
        .map_or(0, |slot| usize::from(slot.0));
    let main = projection.and_then(|p| p.main_hand.as_ref()).or_else(|| {
        world
            .get::<MainInventory>(actor)
            .and_then(|inventory| inventory.0.item(held))
    });
    let off = projection
        .and_then(|p| p.off_hand.as_ref())
        .or_else(|| world.get::<OffhandSlot>(actor).map(|offhand| &offhand.0));
    let armour = world.get::<ArmourInventory>(actor);
    let slots = [
        main,
        off,
        projection
            .and_then(|p| p.armour.as_ref().map(|armour| &armour[0]))
            .or_else(|| armour.map(|armour| armour.helmet())),
        projection
            .and_then(|p| p.armour.as_ref().map(|armour| &armour[1]))
            .or_else(|| armour.map(|armour| armour.chestplate())),
        projection
            .and_then(|p| p.armour.as_ref().map(|armour| &armour[2]))
            .or_else(|| armour.map(|armour| armour.leggings())),
        projection
            .and_then(|p| p.armour.as_ref().map(|armour| &armour[3]))
            .or_else(|| armour.map(|armour| armour.boots())),
    ];
    let items = world.get_resource::<ItemRegistryResource>();
    let block_ids = world
        .get_resource::<ServerConfigResource>()
        .map_or(BlockNetworkIds::default(), |config| {
            config.0.block_network_ids
        });
    slots.map(|slot| match (slot, items) {
        (Some(stack), Some(items)) => {
            crate::server::game::item_descriptor(stack, &items.0, block_ids).unwrap_or_default()
        }
        _ => WireItem::default(),
    })
}

/// Capture only complete, finite, explicitly spawned player actors.
fn views(world: &mut World, ready: &HashSet<Entity>) -> Vec<PlayerView> {
    let actors = world
        .query_filtered::<Entity, With<Player>>()
        .iter(world)
        .filter(|actor| ready.contains(actor))
        .collect::<Vec<_>>();
    actors
        .into_iter()
        .filter_map(|actor| {
            let position = *world.get::<Position>(actor)?;
            if !position.0.is_finite() || !position.0.as_vec3().is_finite() {
                return None;
            }
            Some(PlayerView {
                actor,
                runtime_id: world.get::<RuntimeEntityId>(actor)?.0,
                uuid: world.get::<PlayerUuid>(actor)?.0,
                name: world.get::<PlayerName>(actor)?.0.clone(),
                position,
                rotation: *world.get::<Rotation>(actor)?,
                mode: *world.get::<GameMode>(actor)?,
                tick: input_tick_of(world, actor),
                on_ground: world
                    .get::<PlayerInput>(actor)
                    .is_none_or(|input| input.on_ground),
                audience: world
                    .get::<ActorAudience>(actor)
                    .cloned()
                    .unwrap_or_default(),
                metadata: metadata(world, actor),
                equipment: equipment(world, actor),
                selected: world.get::<HeldSlot>(actor).map_or(0, |slot| slot.0),
            })
        })
        .collect()
}

/// Remember each queued packet separately so small channels cannot starve armour.
fn send_equipment(session: &PlayerSession, view: &PlayerView, known: &mut KnownPlayer) {
    let rid = ActorRuntimeId {
        actor_runtime_id: view.runtime_id as u64,
    };
    for (index, container_id) in [(0, 0), (1, 119)] {
        if known.hands[index].as_ref() == Some(&view.equipment[index])
            && (index != 0 || known.selected == view.selected)
        {
            continue;
        }
        if session.send(McpePacket::from(MobEquipmentPacket {
            target_runtime_id: rid.clone(),
            item: view.equipment[index].clone(),
            slot: if index == 0 { view.selected } else { 0 },
            selected_slot: if index == 0 { view.selected } else { 0 },
            container_id,
        })) {
            known.hands[index] = Some(view.equipment[index].clone());
            if index == 0 {
                known.selected = view.selected;
            }
        }
    }
    let armour = std::array::from_fn(|slot| view.equipment[slot + 2].clone());
    if known.armour.as_ref() == Some(&armour) {
        return;
    }
    if session.send(McpePacket::from(MobArmorEquipmentPacket {
        target_runtime_id: rid,
        head: view.equipment[2].clone(),
        torso: view.equipment[3].clone(),
        legs: view.equipment[4].clone(),
        feet: view.equipment[5].clone(),
        body: WireItem::default(),
    })) {
        known.armour = Some(armour);
    }
}

/// Reconcile ready players against each live viewer and remember only queued state.
pub fn sync_player_views(world: &mut World) {
    let mut cache = world
        .remove_resource::<PlayerViewCache>()
        .unwrap_or_default();
    if let Some(messages) = world.get_resource::<Messages<PlayerSpawnedEvent>>() {
        cache
            .ready
            .extend(cache.spawned.read(messages).map(|event| event.entity));
    }
    if let Some(messages) = world.get_resource::<Messages<PlayerDespawnedEvent>>() {
        for event in cache.despawned.read(messages) {
            cache.ready.remove(&event.entity);
        }
    }
    cache
        .ready
        .retain(|actor| world.get::<Player>(*actor).is_some());
    let current = views(world, &cache.ready);
    let viewers = world
        .query_filtered::<Entity, (With<PlayerSession>, With<Player>)>()
        .iter(world)
        .filter(|viewer| cache.ready.contains(viewer) && !crate::server::game::player_events::initial_state_pending(world,*viewer))
        .collect::<Vec<_>>();
    let allowed = current
        .iter()
        .flat_map(|view| {
            viewers
                .iter()
                .copied()
                .filter(|viewer| *viewer != view.actor && view.audience.allows(view.actor, *viewer))
                .map(|viewer| (viewer, view.actor))
        })
        .collect::<HashSet<_>>();
    cache.known.retain(|&(viewer, actor), known| {
        let Some(session) = world.get::<PlayerSession>(viewer) else {
            return false;
        };
        if !known.connection.matches(session) {
            return false;
        }
        if !allowed.contains(&(viewer, actor)) {
            return !session.send(McpePacket::from(build_remove_actor_packet(
                known.runtime_id,
            )));
        }
        true
    });
    for view in &current {
        for viewer in viewers
            .iter()
            .copied()
            .filter(|viewer| *viewer != view.actor && view.audience.allows(view.actor, *viewer))
        {
            let session = world
                .get::<PlayerSession>(viewer)
                .expect("captured live viewer");
            let key = (viewer, view.actor);
            if cache.known.get(&key).is_some_and(|known| {
                known.runtime_id != view.runtime_id
                    || known.uuid != view.uuid
                    || known.name != view.name
                    || known.mode != view.mode
            }) {
                let known = cache.known.get(&key).unwrap();
                if !session.send(McpePacket::from(build_remove_actor_packet(
                    known.runtime_id,
                ))) {
                    continue;
                }
                cache.known.remove(&key);
            }
            if let std::collections::hash_map::Entry::Vacant(entry) = cache.known.entry(key) {
                let mut packet = build_add_player_packet(
                    view.runtime_id,
                    view.uuid,
                    &view.name,
                    &view.position,
                    &view.rotation,
                    view.mode,
                );
                packet.carried_item = view.equipment[0].clone();
                packet.abilities_data = crate::server::game::join::abilities_packet(
                    view.runtime_id,
                    view.mode,
                    world.get::<crate::command::Operator>(view.actor).is_some(),
                )
                .data;
                packet.entity_data = SynchedActorDataCopyableDataList {
                    data: view.metadata.clone(),
                };
                if !session.send(McpePacket::from(packet)) {
                    continue;
                }
                entry.insert(KnownPlayer {
                    connection: Connection::of(session),
                    runtime_id: view.runtime_id,
                    uuid: view.uuid,
                    name: view.name.clone(),
                    mode: view.mode,
                    last: LastBroadcastPosition {
                        x: view.position.0.x,
                        y: view.position.0.y,
                        z: view.position.0.z,
                        yaw: view.rotation.yaw,
                        pitch: view.rotation.pitch,
                    },
                    metadata: view.metadata.clone(),
                    hands: [None, None],
                    armour: None,
                    selected: view.selected,
                });
            }
            let known = cache.known.get_mut(&key).expect("queued player spawn");
            send_equipment(session, view, known);
            if known.metadata != view.metadata
                && session.send(McpePacket::from(SetActorDataPacket {
                    target_runtime_id: ActorRuntimeId {
                        actor_runtime_id: view.runtime_id as u64,
                    },
                    actor_data: SynchedActorDataCopyableDataList {
                        data: view.metadata.clone(),
                    },
                    synched_properties: PropertySyncData::default(),
                    tick: view.tick.clone(),
                }))
            {
                known.metadata = view.metadata.clone();
            }
            let near = world.get::<Position>(viewer).is_none_or(|position| {
                let a: glam::IVec3 = (position.0.floor().as_ivec3()) >> 4;
                let b: glam::IVec3 = view.position.0.floor().as_ivec3() >> 4;
                (a.x - b.x).abs() <= 1 && (a.z - b.z).abs() <= 1
            });
            if near
                && (super::position_distance_sq(&known.last, &view.position)
                    > super::MOVEMENT_THRESHOLD_SQ
                    || (known.last.yaw - view.rotation.yaw).abs() > super::ROTATION_THRESHOLD
                    || (known.last.pitch - view.rotation.pitch).abs() > super::ROTATION_THRESHOLD)
                && session.send(McpePacket::from(build_move_player_packet(
                    view.runtime_id,
                    &view.position,
                    &view.rotation,
                    view.on_ground,
                    view.tick.clone(),
                )))
            {
                known.last = LastBroadcastPosition {
                    x: view.position.0.x,
                    y: view.position.0.y,
                    z: view.position.0.z,
                    yaw: view.rotation.yaw,
                    pitch: view.rotation.pitch,
                };
            }
        }
        // Preserve the public aggregate position marker; per-viewer retry state
        // remains independent when a particular outbound channel is full.
        if let Some(mut last) = world.get_mut::<LastBroadcastPosition>(view.actor)
            && (super::position_distance_sq(&last, &view.position) > super::MOVEMENT_THRESHOLD_SQ
                || (last.yaw - view.rotation.yaw).abs() > super::ROTATION_THRESHOLD
                || (last.pitch - view.rotation.pitch).abs() > super::ROTATION_THRESHOLD)
        {
            *last = LastBroadcastPosition {
                x: view.position.0.x,
                y: view.position.0.y,
                z: view.position.0.z,
                yaw: view.rotation.yaw,
                pitch: view.rotation.pitch,
            };
        }
    }
    world.insert_resource(cache);
}

#[cfg(test)]
mod tests;
