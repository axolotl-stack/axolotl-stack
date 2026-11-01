//! Exact-quantity grants and durable ownership of their overflow item actors.

use bevy_ecs::prelude::*;
use glam::DVec3;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, mpsc};
use uuid::Uuid;

use super::inventory_effects::{self, EffectError, InventoryEffect, InventoryState, Submission};
use super::types::{ItemRegistryResource, PlayerProviderResource};
use crate::entity::components::{
    GameMode, Health, ItemEntityStack, MainInventory, PickupDelay, Player, PlayerUuid, Position,
};
use crate::item::ItemStack;
use crate::storage::{
    StoredItemStack,
    item_grants::{ItemMutation, PersistentItem},
};

/// Positional strategies fill only an empty requested slot. Occupied slots and
/// quantity exceeding that single slot go directly to persistent world items.
#[derive(Clone, Copy, Debug, serde::Serialize)]
pub(crate) enum Placement {
    Any,
    EmptyMainOrDrop(u8),
    EmptyArmourOrDrop(u8),
    EmptyOffhandOrDrop,
}

#[derive(Clone, Debug)]
pub(crate) struct Grant {
    pub item: ItemStack,
    pub quantity: u32,
    pub placement: Placement,
}

pub(crate) struct Planned {
    pub expected: InventoryState,
    pub replacement: InventoryState,
    pub items: Vec<PersistentItem>,
}

/// Hash the logical request, not a new actor's position or its current inventory.
pub(crate) fn intent(
    world: Uuid,
    dimension: i32,
    grants: &[Grant],
) -> Result<[u8; 32], EffectError> {
    validate(grants)?;
    let entries: Vec<_> = grants
        .iter()
        .map(|grant| {
            (
                StoredItemStack::from(&grant.item.clone().with_count(1)),
                grant.quantity,
                grant.placement,
            )
        })
        .collect();
    let bytes = serde_json::to_vec(&(world, dimension, entries))
        .map_err(|error| EffectError::InvalidInventory(error.to_string()))?;
    let mut hash = Sha256::new();
    hash.update(b"unastar.item-grant.v1\0");
    hash.update(bytes);
    Ok(hash.finalize().into())
}

fn invalid() -> EffectError {
    EffectError::InvalidInventory("invalid item grant".into())
}

fn validate(grants: &[Grant]) -> Result<(), EffectError> {
    if grants.is_empty() || grants.len() > 128 {
        return Err(invalid());
    }
    let mut total = 0u32;
    for grant in grants {
        total = total.checked_add(grant.quantity).ok_or_else(invalid)?;
        if grant.quantity == 0
            || total > 16_384
            || grant.item.is_empty()
            || grant.item.max_stack_size() == 0
            || matches!(grant.placement, Placement::EmptyMainOrDrop(slot) if slot >= 36)
            || matches!(grant.placement, Placement::EmptyArmourOrDrop(slot) if slot >= 4 || !crate::item::equipment::accepts_armour(&grant.item, usize::from(slot)))
            || grant
                .item
                .nbt
                .as_ref()
                .is_some_and(|bytes| bytes.len() > 131_072)
        {
            return Err(invalid());
        }
    }
    Ok(())
}

fn fill(slot: &mut ItemStack, item: &ItemStack, remaining: &mut u32) {
    if *remaining == 0 || (!slot.is_empty() && !slot.comparable(item)) {
        return;
    }
    let old = if slot.is_empty() { 0 } else { slot.count };
    let count = (*remaining).min(u32::from(item.max_stack_size().saturating_sub(old))) as u8;
    if count != 0 {
        *slot = item.clone().with_count(old + count);
        *remaining -= u32::from(count);
    }
}

/// Plan against a complete genuine capture before quarantining or scheduling IO.
pub(crate) fn plan(
    world: &World,
    player: Entity,
    key: &str,
    grants: &[Grant],
) -> Result<Planned, EffectError> {
    validate(grants)?;
    if !world
        .get_resource::<Runtime>()
        .is_some_and(|runtime| runtime.initialized && !runtime.failed)
    {
        return Err(EffectError::Busy);
    }
    let identity = world
        .get_resource::<crate::world::WorldIdentity>()
        .ok_or(EffectError::UnsupportedProvider)?;
    let feet = world
        .get::<Position>(player)
        .ok_or(EffectError::MissingPlayer)?
        .0
        - DVec3::Y * crate::entity::PLAYER_EYE_HEIGHT;
    let registry = &world
        .get_resource::<ItemRegistryResource>()
        .ok_or(EffectError::MissingProvider)?
        .0;
    let expected = InventoryState::capture(world, player)?;
    let restore = |stack: &StoredItemStack| stack.clone().restore(registry).map_err(|_| invalid());
    let mut main: Vec<_> = expected
        .main
        .iter()
        .map(restore)
        .collect::<Result<_, _>>()?;
    if main.len() != 36 {
        return Err(invalid());
    }
    let mut armour: Vec<_> = expected
        .armour
        .iter()
        .map(restore)
        .collect::<Result<_, _>>()?;
    let mut offhand = restore(&expected.offhand)?;
    let mut items = Vec::new();
    for grant in grants {
        let mut remaining = grant.quantity;
        match grant.placement {
            Placement::Any => (),
            Placement::EmptyMainOrDrop(slot) => {
                let slot = &mut main[usize::from(slot)];
                if slot.is_empty() {
                    fill(slot, &grant.item, &mut remaining);
                }
            }
            Placement::EmptyArmourOrDrop(slot) => {
                if !crate::item::equipment::accepts_armour(&grant.item, usize::from(slot)) {
                    return Err(invalid());
                }
                let slot = &mut armour[usize::from(slot)];
                if slot.is_empty() {
                    fill(slot, &grant.item, &mut remaining);
                }
            }
            Placement::EmptyOffhandOrDrop => {
                if offhand.is_empty() {
                    fill(&mut offhand, &grant.item, &mut remaining);
                }
            }
        }
        // Only the unpositioned strategy searches other carried slots.
        if matches!(grant.placement, Placement::Any) {
            for slot in main.iter_mut().filter(|slot| !slot.is_empty()) {
                fill(slot, &grant.item, &mut remaining);
            }
            for slot in main.iter_mut().filter(|slot| slot.is_empty()) {
                fill(slot, &grant.item, &mut remaining);
            }
        }
        while remaining != 0 {
            if items.len() >= 256 {
                return Err(invalid());
            }
            let count = remaining.min(u32::from(grant.item.max_stack_size())) as u8;
            let mut hash = Sha256::new();
            hash.update(b"unastar.grant-drop.v1\0");
            hash.update(identity.id.as_bytes());
            hash.update(
                world
                    .get::<PlayerUuid>(player)
                    .ok_or(EffectError::MissingPlayer)?
                    .0
                    .as_bytes(),
            );
            hash.update((key.len() as u64).to_be_bytes());
            hash.update(key.as_bytes());
            hash.update((items.len() as u32).to_be_bytes());
            let bytes: [u8; 32] = hash.finalize().into();
            let id = Uuid::from_bytes(bytes[..16].try_into().expect("fixed digest"));
            items.push(PersistentItem {
                id,
                world: identity.id,
                dimension: identity.dimension,
                feet: feet.to_array(),
                stack: StoredItemStack::from(&grant.item.clone().with_count(count)),
            });
            remaining -= u32::from(count);
        }
    }
    let mut replacement = expected.clone();
    replacement.main = main.iter().map(StoredItemStack::from).collect();
    replacement.armour = std::array::from_fn(|slot| StoredItemStack::from(&armour[slot]));
    replacement.offhand = StoredItemStack::from(&offhand);
    Ok(Planned {
        expected,
        replacement,
        items,
    })
}

/// A durable item cannot participate in unjournaled pickup, merge or removal paths.
pub(crate) use crate::entity::components::PersistentWorldItem as DurableItem;

type Retirement = (Uuid, crate::storage::StorageResult<()>);
type RetirementReceiver = Mutex<mpsc::Receiver<Retirement>>;

#[derive(Resource, Default)]
struct Runtime {
    actors: HashMap<Uuid, Entity>,
    pending: HashSet<Uuid>,
    pickups: HashMap<String, Uuid>,
    retire_tx: Option<mpsc::Sender<Retirement>>,
    retire_rx: Option<RetirementReceiver>,
    initialized: bool,
    failed: bool,
    receiver: Option<Mutex<mpsc::Receiver<crate::storage::StorageResult<Vec<PersistentItem>>>>>,
}

/// Materialize genuine native item actors only from acknowledged ownership rows.
fn spawn(world: &mut World, item: &PersistentItem) {
    if item.stack.count == 0 || world.resource::<Runtime>().actors.contains_key(&item.id) {
        return;
    }
    let Some(identity) = world.get_resource::<crate::world::WorldIdentity>() else {
        return;
    };
    if identity.id != item.world || identity.dimension != item.dimension {
        return;
    }
    let Some(registry) = world.get_resource::<ItemRegistryResource>() else {
        return;
    };
    let Ok(stack) = item.stack.clone().restore(&registry.0) else {
        // Invalid persisted native values must not be silently discarded or replaced.
        fail(world);
        return;
    };
    if let Some(entity) =
        crate::entity::spawn_item_drop(world, DVec3::from_array(item.feet), stack, None)
    {
        world.entity_mut(entity).insert(DurableItem(item.clone()));
        world
            .resource_mut::<Runtime>()
            .actors
            .insert(item.id, entity);
    }
}

/// Called before the corresponding inventory completion notification is emitted.
pub(crate) fn publish(world: &mut World, mutation: &ItemMutation, collector: Option<Entity>) {
    world.init_resource::<Runtime>();
    match mutation {
        ItemMutation::Create(items) => {
            for item in items {
                spawn(world, item);
            }
        }
        ItemMutation::Take {
            expected,
            remaining,
        } => {
            let entity = world
                .resource_mut::<Runtime>()
                .actors
                .get(&expected.id)
                .copied();
            if let Some(entity) = entity {
                if *remaining == 0 {
                    remove_actor(world, entity, collector);
                    world.resource_mut::<Runtime>().actors.remove(&expected.id);
                } else if let Some(mut stack) = world.get_mut::<ItemEntityStack>(entity) {
                    stack.0.count = *remaining;
                    if let Some(mut item) = world.get_mut::<DurableItem>(entity) {
                        item.0.stack.count = *remaining;
                    }
                }
            }
            world.resource_mut::<Runtime>().pending.remove(&expected.id);
            world
                .resource_mut::<Runtime>()
                .pickups
                .retain(|_, id| *id != expected.id);
        }
    }
}

pub(crate) fn poll(world: &mut World) {
    if !world.contains_resource::<Runtime>() {
        world.init_resource::<Runtime>();
        world.add_observer(
            |event: On<inventory_effects::InventoryEffectFailed>, mut runtime: ResMut<Runtime>| {
                if event.kind == crate::storage::player_writer::WriteFailureKind::NotApplied
                    && let Some(id) = runtime.pickups.remove(&event.id)
                {
                    runtime.pending.remove(&id);
                }
            },
        );
        let (tx, rx) = mpsc::channel();
        let mut runtime = world.resource_mut::<Runtime>();
        runtime.retire_tx = Some(tx);
        runtime.retire_rx = Some(Mutex::new(rx));
    }
    if world.resource::<Runtime>().failed {
        return;
    }
    if !world.resource::<Runtime>().initialized && world.resource::<Runtime>().receiver.is_none() {
        let Some(identity) = world.get_resource::<crate::world::WorldIdentity>() else {
            return;
        };
        let Some(provider) = world
            .get_resource::<PlayerProviderResource>()
            .map(|provider| provider.0.clone())
            .filter(|provider| provider.atomic_durable_items())
        else {
            return;
        };
        let Ok(handle) = tokio::runtime::Handle::try_current() else {
            return;
        };
        let (id, dimension) = (identity.id, identity.dimension);
        let (tx, rx) = mpsc::channel();
        world.resource_mut::<Runtime>().receiver = Some(Mutex::new(rx));
        handle.spawn(async move {
            let _ = tx.send(provider.world_items(id, dimension).await);
        });
    }
    let loaded = world
        .resource::<Runtime>()
        .receiver
        .as_ref()
        .and_then(|rx| rx.lock().ok()?.try_recv().ok());
    if let Some(loaded) = loaded {
        world.resource_mut::<Runtime>().receiver = None;
        world.resource_mut::<Runtime>().initialized = true;
        match loaded {
            Ok(items) => {
                for item in items {
                    spawn(world, &item);
                }
            }
            Err(_) => fail(world),
        }
    }
    retire_expired(world);
    // Pickup is itself an atomic inventory + conditional item-row effect.
    if world.resource::<Runtime>().initialized {
        pickup(world);
    }
}

fn pickup(world: &mut World) {
    let items: Vec<_> = world
        .query::<(Entity, &Position, &DurableItem, &PickupDelay)>()
        .iter(world)
        .filter(|(_, _, item, delay)| {
            delay.can_pickup() && !world.resource::<Runtime>().pending.contains(&item.0.id)
        })
        .map(|(entity, position, item, _)| (entity, position.0, item.0.clone()))
        .collect();
    let players: Vec<_> = world
        .query_filtered::<(Entity, &Position, &Health, &GameMode), With<Player>>()
        .iter(world)
        .filter(|(entity, _, health, mode)| {
            !health.is_dead()
                && **mode != GameMode::Spectator
                && !super::player_events::initial_state_pending(world, *entity)
        })
        .map(|(entity, position, _, mode)| (entity, position.0, *mode))
        .collect();
    for (_, position, item) in items {
        for &(player, eye, mode) in &players {
            if !super::items::player_box(eye)
                .inflate(super::items::PICKUP_REACH)
                .intersects(&crate::registry::collision::Aabb::from_feet(
                    position, 0.25, 0.25,
                ))
            {
                continue;
            }
            let Ok(expected) = InventoryState::capture(world, player) else {
                continue;
            };
            let Some(mut inventory) = world.get::<MainInventory>(player).cloned() else {
                continue;
            };
            let Some(stack) = world
                .get_resource::<ItemRegistryResource>()
                .and_then(|registry| item.stack.clone().restore(&registry.0).ok())
            else {
                continue;
            };
            let (added, leftover) = inventory.0.add_item(stack);
            if added == 0 && mode != GameMode::Creative {
                continue;
            }
            let remaining = if mode == GameMode::Creative {
                0
            } else {
                leftover.count
            };
            let mut replacement = expected.clone();
            replacement.main = inventory
                .0
                .slots()
                .iter()
                .map(StoredItemStack::from)
                .collect();
            let Some(uuid) = world.get::<PlayerUuid>(player).map(|uuid| uuid.0) else {
                continue;
            };
            let id = format!("pickup:{}:{}:{uuid}", item.id.simple(), item.stack.count);
            let Ok(hash) = inventory_effects::request_hash(&replacement) else {
                continue;
            };
            let mutation = ItemMutation::Take {
                expected: item.clone(),
                remaining,
            };
            let operation = id.clone();
            if matches!(
                inventory_effects::submit_items(
                    world,
                    player,
                    InventoryEffect {
                        id,
                        expected,
                        replacement
                    },
                    hash,
                    mutation,
                    Arc::new(())
                ),
                Ok(Submission::Queued)
            ) {
                world.resource_mut::<Runtime>().pending.insert(item.id);
                world
                    .resource_mut::<Runtime>()
                    .pickups
                    .insert(operation, item.id);
                break;
            }
        }
    }
}

fn fail(world: &mut World) {
    world.resource_mut::<Runtime>().failed = true;
    crate::plugin::manager::readiness::fence_world(world);
}

pub(crate) fn recovery_required(world: &World) -> bool {
    world
        .get_resource::<Runtime>()
        .is_some_and(|runtime| runtime.failed)
}

fn retire_expired(world: &mut World) {
    use crate::entity::components::DespawnTimer;
    let done: Vec<_> = world
        .resource::<Runtime>()
        .retire_rx
        .as_ref()
        .and_then(|rx| rx.lock().ok())
        .map(|rx| rx.try_iter().collect())
        .unwrap_or_default();
    for (id, result) in done {
        if result.is_err() {
            fail(world);
            return;
        }
        let entity = world.resource_mut::<Runtime>().actors.remove(&id);
        if let Some(entity) = entity {
            remove_actor(world, entity, None);
        }
        world.resource_mut::<Runtime>().pending.remove(&id);
    }
    let floor = f64::from(crate::world::chunk::MIN_Y)
        - crate::entity::systems::lifecycle::OUT_OF_WORLD_DEPTH;
    let expired: Vec<_> = world
        .query::<(&Position, &DurableItem, &mut DespawnTimer)>()
        .iter_mut(world)
        .filter_map(|(position, item, mut timer)| {
            timer.tick();
            (timer.should_despawn() || position.0.y < floor).then(|| item.0.clone())
        })
        .collect();
    for item in expired {
        if world.resource::<Runtime>().pending.contains(&item.id) {
            continue;
        }
        let Some(tx) = world.resource::<Runtime>().retire_tx.clone() else {
            continue;
        };
        let Some(provider) = world
            .get_resource::<PlayerProviderResource>()
            .map(|p| p.0.clone())
        else {
            continue;
        };
        let Ok(handle) = tokio::runtime::Handle::try_current() else {
            continue;
        };
        world.resource_mut::<Runtime>().pending.insert(item.id);
        handle.spawn(async move {
            let result = provider.retire_world_item(&item).await;
            let _ = tx.send((item.id, result));
        });
    }
}

pub(crate) fn ready(world: &World) -> bool {
    world
        .get_resource::<Runtime>()
        .is_some_and(|runtime| runtime.initialized && !runtime.failed)
}

fn remove_actor(world: &mut World, entity: Entity, collector: Option<Entity>) {
    use crate::entity::components::{ActorViewers, PlayerSession, RuntimeEntityId, RuntimeId};
    use jolyne::valentine::types::{ActorRuntimeId, ActorUniqueId};
    use jolyne::valentine::{McpePacket, RemoveActorPacket, TakeItemActorPacket};
    let runtime = world.get::<RuntimeId>(entity).map(|id| id.0);
    let mut viewers = world
        .get::<ActorViewers>(entity)
        .map(|viewers| viewers.viewers.clone())
        .unwrap_or_default();
    if let Some(collector) = collector
        && !viewers.contains(&collector)
    {
        viewers.push(collector);
    }
    if let Some(runtime) = runtime {
        for viewer in viewers {
            let packet = collector
                .and_then(|player| {
                    let id = world.get::<RuntimeEntityId>(player)?.0;
                    let allowed = world
                        .get::<crate::entity::components::ActorAudience>(player)
                        .is_none_or(|audience| audience.allows(player, viewer));
                    allowed.then(|| {
                        McpePacket::from(TakeItemActorPacket {
                            item_runtime_id: ActorRuntimeId {
                                actor_runtime_id: runtime as u64,
                            },
                            actor_runtime_id: ActorRuntimeId {
                                actor_runtime_id: id as u64,
                            },
                        })
                    })
                })
                .unwrap_or_else(|| {
                    McpePacket::from(RemoveActorPacket {
                        target_actor_id: ActorUniqueId {
                            actor_unique_id: runtime,
                        },
                    })
                });
            if let Some(session) = world.get::<PlayerSession>(viewer) {
                let _ = session.send(packet);
            }
        }
    }
    world.despawn(entity);
}
