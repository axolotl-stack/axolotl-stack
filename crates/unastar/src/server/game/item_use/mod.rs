//! Using the held item in the air: eating, drinking, throwing, bows,
//! crossbows and tridents.
//!
//! The client starts using an item with an `ItemUse` transaction of action
//! `Use` (click air). Like BDS 1.26.51 (capture `item-use-20261029`), the
//! server finishes eating and drinking by itself once the item's use
//! duration has passed in server ticks; an `ItemRelease` of action `Use`
//! (consume) finishes it at once, even early, and `Release` or selecting
//! another hotbar slot cancels it. The item is always the one the server
//! has in the selected hotbar slot; the client's item description is not
//! trusted.

pub mod food;
pub mod on_block;
pub mod ranged;

use bevy_ecs::prelude::*;
use jolyne::valentine::types::{
    ActorRuntimeId, DataItemEntry, DataItemEntryPayload, DataItemInt64Payload, EnumsDataItemType,
    PlayerInputTick, PropertySyncData, SynchedActorDataCopyableDataList,
};
use jolyne::valentine::{
    CompletedUsingItemPacket, McpePacket, SetActorDataPacket, actor_data_i_ds, actor_flags,
};

use crate::ecs::events::{EventBuffer, ServerEvent};
use crate::entity::components::{
    GameMode, Health, HeldSlot, MainInventory, PlayerInput, PlayerSession, PlayerState, Position,
    RuntimeEntityId,
};
use crate::item::ItemStack;
use crate::server::broadcast::EntityGrid;

/// An air-use packet of one player, in arrival order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UseRequest {
    /// `ItemUse` action `Use`: start using, or finish eating.
    UseAir,
    /// `ItemRelease` action `Release`: let go of a drawn bow or trident.
    Release,
    /// `ItemRelease` action `Use` (consume).
    Consume,
}

/// Air-use packets waiting for [`apply_item_use`].
#[derive(Resource, Debug, Default)]
pub struct ItemUseQueue(pub Vec<(Entity, UseRequest)>);

/// The item a player is using (eating, drawing, charging).
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
#[component(storage = "SparseSet")]
pub struct ItemUsing {
    /// Hotbar slot of the used item.
    pub slot: u8,
    /// Registry id of the used item.
    pub item: u32,
    /// Server tick using started.
    pub since: u64,
}

/// What `UseAir` does with an item.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum UseKind {
    Consume(food::Consumable),
    Draw(ranged::Drawn),
    Throw,
}

/// How an item is used in the air, from its data.
pub fn use_kind(stack: &ItemStack) -> Option<UseKind> {
    let entry = stack.item()?;
    if let Some(consumable) = food::consumable(entry) {
        return Some(UseKind::Consume(consumable));
    }
    if let Some(drawn) = ranged::drawn(entry.identifier()) {
        return Some(UseKind::Draw(drawn));
    }
    ranged::thrown(entry.identifier()).map(|_| UseKind::Throw)
}

/// The selected hotbar slot and its stack.
pub fn held(world: &World, player: Entity) -> Option<(u8, ItemStack)> {
    let slot = world.get::<HeldSlot>(player)?.0;
    let stack = world
        .get::<MainInventory>(player)?
        .0
        .item(usize::from(slot))?
        .clone();
    (!stack.is_empty()).then_some((slot, stack))
}

/// Replace the stack in hotbar `slot` and tell the client.
pub fn set_slot(world: &mut World, player: Entity, slot: u8, stack: ItemStack) {
    let Some(stored) = world.get_mut::<MainInventory>(player).and_then(|mut inv| {
        inv.0.set_item(usize::from(slot), stack).ok()?;
        inv.0.item(usize::from(slot)).cloned()
    }) else {
        return;
    };
    super::send_slot(world, player, usize::from(slot), &stored);
}

/// Put `stack` into the player's inventory, dropping what does not fit at
/// the player's feet as a thrown item.
pub fn give_or_drop(world: &mut World, player: Entity, stack: ItemStack) {
    if stack.is_empty() {
        return;
    }
    let Some((before, leftover, after)) = world.get_mut::<MainInventory>(player).map(|mut inv| {
        let before = inv.0.slots().to_vec();
        let (_, leftover) = inv.0.add_item(stack.clone());
        (before, leftover, inv.0.slots().to_vec())
    }) else {
        return;
    };
    for (slot, (old, new)) in before.iter().zip(&after).enumerate() {
        if old != new {
            super::send_slot(world, player, slot, new);
        }
    }
    if !leftover.is_empty() {
        let thrower = crate::entity::item_drop::drop_thrower_id(world, player).map(|_| player);
        if let Some(feet) = world.get::<Position>(player).map(|p| p.0) {
            crate::entity::spawn_item_drop(world, feet, leftover, thrower);
        }
    }
}

/// Players near `player` (within a chunk), including `player`.
pub fn nearby_players(world: &World, player: Entity) -> Vec<Entity> {
    let Some(position) = world.get::<Position>(player) else {
        return Vec::new();
    };
    let chunk = (
        (position.0.x.floor() as i32) >> 4,
        (position.0.z.floor() as i32) >> 4,
    );
    let mut out = world
        .get_resource::<EntityGrid>()
        .map(|grid| grid.get_neighbors(chunk, 1))
        .unwrap_or_default();
    out.retain(|entity| world.get::<PlayerSession>(*entity).is_some());
    if !out.contains(&player) {
        out.push(player);
    }
    out
}

/// Send `packet` to `players`.
pub fn send_to(world: &World, players: &[Entity], packet: &McpePacket) {
    for viewer in players {
        if let Some(session) = world.get::<PlayerSession>(*viewer) {
            let _ = session.send(packet.clone());
        }
    }
}

/// The player's own input tick.
fn input_tick(world: &World, player: Entity) -> i64 {
    world
        .get::<PlayerInput>(player)
        .map_or(0, |input| input.tick)
}

/// The current server tick.
fn server_tick(world: &World) -> u64 {
    world
        .get_resource::<crate::ecs::resources::TickCounter>()
        .map_or(0, |t| t.current)
}

/// Whether a player may use items at all.
fn can_use(world: &World, player: Entity) -> bool {
    world
        .get::<GameMode>(player)
        .is_some_and(|mode| *mode != GameMode::Spectator)
        && world.get::<Health>(player).is_some_and(|h| !h.is_dead())
}

/// The flag word BDS sends for a player using an item: CANCLIMB,
/// BREATHING, HAS_COLLISION and HAS_GRAVITY plus the movement state and
/// USINGITEM (capture `item-use-20261029`).
fn player_flags(world: &World, player: Entity, using: bool) -> i64 {
    let mut flags = (1u64 << actor_flags::CANCLIMB)
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
            }
        }
    }
    if using {
        flags |= 1u64 << actor_flags::USINGITEM;
    }
    flags as i64
}

/// Tell the player and the players near it that it started or stopped
/// using an item (BDS sends the flag word to the user too).
fn broadcast_using(world: &World, player: Entity, using: bool) {
    let Some(runtime_id) = world.get::<RuntimeEntityId>(player).map(|r| r.0) else {
        return;
    };
    let packet = McpePacket::from(SetActorDataPacket {
        target_runtime_id: ActorRuntimeId {
            actor_runtime_id: runtime_id as u64,
        },
        actor_data: SynchedActorDataCopyableDataList {
            data: vec![DataItemEntry {
                id: actor_data_i_ds::RESERVED_0,
                payload: DataItemEntryPayload::DataItemInt64Payload(DataItemInt64Payload {
                    type_: EnumsDataItemType::Int64,
                    value: player_flags(world, player, using),
                }),
            }],
        },
        synched_properties: PropertySyncData::default(),
        tick: PlayerInputTick {
            inputtick: u64::try_from(input_tick(world, player)).unwrap_or(0),
        },
    });
    send_to(world, &nearby_players(world, player), &packet);
}

/// Start using the held item.
pub fn start_using(world: &mut World, player: Entity, slot: u8, item: u32) {
    let since = server_tick(world);
    world
        .entity_mut(player)
        .insert(ItemUsing { slot, item, since });
    broadcast_using(world, player, true);
}

/// Stop using whatever the player uses.
pub fn stop_using(world: &mut World, player: Entity) {
    if world.entity_mut(player).take::<ItemUsing>().is_some() {
        broadcast_using(world, player, false);
    }
}

/// Server ticks since the player started using the current item.
pub fn used_ticks(world: &World, using: &ItemUsing) -> u64 {
    server_tick(world).saturating_sub(using.since)
}

/// `CompletedUsingItem` methods BDS sends (capture `item-use-20261029`):
/// 1 for food and -1 for the milk bucket and crossbow charging.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum UseMethod {
    None = -1,
    Eat = 1,
}

/// Tell the player's client the item use finished.
pub fn completed(world: &World, player: Entity, network_id: i32, method: UseMethod) {
    if let (Some(session), Ok(item_id)) = (
        world.get::<PlayerSession>(player),
        i16::try_from(network_id),
    ) {
        let _ = session.send(McpePacket::from(CompletedUsingItemPacket {
            item_id,
            item_use_method: method as i32,
        }));
    }
}

/// System (exclusive, `PacketApplySet`): handle queued air uses in order.
pub fn apply_item_use(world: &mut World) {
    let requests = std::mem::take(&mut world.get_resource_or_init::<ItemUseQueue>().0);
    for (player, request) in requests {
        if !can_use(world, player) {
            stop_using(world, player);
            continue;
        }
        if let Some(mut events) = world.get_resource_mut::<EventBuffer>()
            && request == UseRequest::UseAir
        {
            events.push(ServerEvent::PlayerItemUse { entity: player });
        }
        handle(world, player, request);
    }
}

fn handle(world: &mut World, player: Entity, request: UseRequest) {
    let Some((slot, stack)) = held(world, player) else {
        stop_using(world, player);
        return;
    };
    let Some(item) = stack.item().map(|entry| entry.id) else {
        return;
    };
    // Switching items abandons what was being used.
    let using = world
        .get::<ItemUsing>(player)
        .copied()
        .filter(|using| using.slot == slot && using.item == item);
    if using.is_none() {
        stop_using(world, player);
    }
    match (use_kind(&stack), request, using) {
        (Some(UseKind::Consume(consumable)), UseRequest::UseAir, None) => {
            food::start(world, player, slot, &stack, consumable)
        }
        // Already eating: BDS finishes on its own.
        (Some(UseKind::Consume(_)), UseRequest::UseAir, Some(_)) => {}
        // BDS honours an early consume and finishes at once.
        (Some(UseKind::Consume(consumable)), UseRequest::Consume, Some(_)) => {
            food::finish(world, player, slot, &stack, consumable.kind)
        }
        (Some(UseKind::Draw(drawn)), UseRequest::UseAir, using) => {
            ranged::use_drawn(world, player, slot, &stack, drawn, using)
        }
        (Some(UseKind::Draw(drawn)), UseRequest::Release, using) => {
            ranged::release_drawn(world, player, slot, &stack, drawn, using)
        }
        (Some(UseKind::Throw), UseRequest::UseAir, _) => ranged::throw(world, player),
        (_, UseRequest::Release | UseRequest::Consume, _) => stop_using(world, player),
        (None, UseRequest::UseAir, _) => {}
    }
}

/// `MovePlayer` teleport causes (protocol constants).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum TeleportCause {
    Projectile = 1,
    ChorusFruit = 2,
}

/// Move a player to `feet` and tell its client (teleport mode).
pub fn teleport(world: &mut World, player: Entity, feet: glam::DVec3, cause: TeleportCause) {
    use jolyne::valentine::types::{
        EnumsPlayerPositionModeComponentPositionMode, MovePlayerTeleportData, Vec2, Vec3,
    };
    let eye = feet + glam::DVec3::new(0.0, crate::entity::item_drop::PLAYER_EYE_HEIGHT, 0.0);
    let Some(rotation) = world
        .get::<crate::entity::components::Rotation>(player)
        .copied()
    else {
        return;
    };
    let Some(runtime_id) = world.get::<RuntimeEntityId>(player).map(|r| r.0) else {
        return;
    };
    if let Some(mut position) = world.get_mut::<Position>(player) {
        position.0 = eye;
    }
    let packet = McpePacket::from(jolyne::valentine::MovePlayerPacket {
        player_runtime_id: ActorRuntimeId {
            actor_runtime_id: runtime_id as u64,
        },
        position: Vec3 {
            x: eye.x as f32,
            y: eye.y as f32,
            z: eye.z as f32,
        },
        rotation: Vec2 {
            x: rotation.pitch,
            y: rotation.yaw,
        },
        y_head_rotation: rotation.head_yaw,
        position_mode: EnumsPlayerPositionModeComponentPositionMode::Teleport,
        on_ground: false,
        riding_runtime_id: ActorRuntimeId {
            actor_runtime_id: 0,
        },
        teleport_data: Some(MovePlayerTeleportData {
            teleportation_cause: cause as i32,
            // Bedrock's actor type id of the player.
            source_actor_type: 63,
        }),
        tick: PlayerInputTick {
            inputtick: u64::try_from(input_tick(world, player)).unwrap_or(0),
        },
    });
    // Other players follow through the regular position broadcast.
    send_to(world, &[player], &packet);
}

/// System (exclusive): finish eating and drinking whose use duration has
/// passed, drop uses whose item left the selected slot, and every 4 ticks
/// show the other nearby players the eating particles (`ActorEvent` `Feed`
/// with the item's id and aux; the eater's client draws its own).
pub fn tick_using(
    world: &mut World,
    users: &mut QueryState<(Entity, &ItemUsing)>,
    mut due: Local<Vec<(Entity, ItemUsing)>>,
) {
    due.extend(users.iter(world).map(|(entity, using)| (entity, *using)));
    let tick = server_tick(world);
    for (player, using) in due.drain(..) {
        let current = held(world, player).filter(|(slot, stack)| {
            *slot == using.slot && stack.item().is_some_and(|e| e.id == using.item)
        });
        let Some((slot, stack)) = current else {
            stop_using(world, player);
            continue;
        };
        let consumable = match use_kind(&stack) {
            Some(UseKind::Consume(consumable)) => consumable,
            Some(UseKind::Draw(drawn)) => {
                ranged::tick_drawn(world, player, drawn, &using);
                continue;
            }
            _ => continue,
        };
        if used_ticks(world, &using) >= u64::from(consumable.duration_ticks) {
            food::finish(world, player, slot, &stack, consumable.kind);
        } else if tick.is_multiple_of(4) {
            feed(world, player, &stack);
        }
    }
}

fn feed(world: &World, player: Entity, stack: &ItemStack) {
    let (Some(runtime_id), Some(entry)) = (
        world.get::<RuntimeEntityId>(player).map(|r| r.0),
        stack.item(),
    ) else {
        return;
    };
    let packet = McpePacket::from(jolyne::valentine::ActorEventPacket {
        target_runtime_id: ActorRuntimeId {
            actor_runtime_id: runtime_id as u64,
        },
        event_id: jolyne::valentine::types::EnumsActorEvent::Feed,
        data: (entry.network_id() << 16) | i32::from(stack.damage),
        fire_at_position: None,
    });
    let viewers: Vec<_> = nearby_players(world, player)
        .into_iter()
        .filter(|viewer| *viewer != player)
        .collect();
    send_to(world, &viewers, &packet);
}

#[cfg(test)]
mod tests;
