//! Block domain system (exclusive).
//!
//! Processes block actions from PlayerAuthInput (break/place/crack) and
//! block clicks from InventoryTransaction. Runs as an exclusive system
//! because it needs `world.trigger()` and `world.write_message()`.

use bevy_ecs::prelude::*;
use glam::IVec3;
use tracing::{debug, trace};

use super::packet_queues::{BlockAction, BlockPacketQueue};
use crate::ecs::events::{EventBuffer, ServerEvent};
use crate::entity::components::{BreakingState, PlayerSession, PlayerUuid};
use crate::world::chunk::blocks;
use crate::world::ecs::{BlockBroadcastEvent, BlockChanged, ChunkManager, ChunkViewers};
use crate::world::ecs::{world_to_chunk_coords, world_to_local_coords};
use jolyne::valentine::types::{BlockPos, EnumsPlayerActionType, Vec3};
use jolyne::valentine::{LevelEventPacket, McpePacket};

// LevelEvent ids, carried as canonical i32 values (the names gophertunnel and
// Dragonfly use).
const LEVEL_EVENT_BLOCK_START_BREAK: i32 = 3600;
const LEVEL_EVENT_BLOCK_STOP_BREAK: i32 = 3601;
const LEVEL_EVENT_BLOCK_BREAK_SPEED: i32 = 3602;
/// Crack particles on one side of a block: this plus the face (down, up,
/// north, south, west, east).
const LEVEL_EVENT_PARTICLES_CRACK_BLOCK: i32 = 3603;
const LEVEL_EVENT_PARTICLES_DESTROY_BLOCK: i32 = 2001;
const BLOCK_UPDATE_NEIGHBOURS: u32 = 1;
const BLOCK_UPDATE_NETWORK: u32 = 1 << 1;

/// Maximum block actions per PlayerAuthInput packet.
const MAX_BLOCK_ACTIONS: usize = 64;
/// Block actions one player may have waiting for the commit of a break
/// (see [`apply_block_actions`]); the inbound cap per session and tick.
const MAX_WAITING_BLOCK_ACTIONS: usize = 64;

/// ECS exclusive system: drain block packet queue and apply block changes.
///
/// Runs in `PacketApplySet` after `apply_movement` (which forwards every
/// accepted PlayerAuthInput, with or without block actions). Exclusive
/// because it fires observers (`BlockChanged`) and writes messages
/// (`BlockBroadcastEvent`).
///
/// Inputs that arrive together are applied together, as BDS does, except
/// that a player's actions after an input that completed a survival break
/// wait for the next tick: plugin dispatch commits the break after this
/// tick, and BDS breaks the block at once, so those inputs must see it gone
/// (in a burst of 16 inputs the 15th broke dirt and the 16th found air).
pub fn apply_block_actions(world: &mut World) {
    let actions: Vec<_> = {
        let Some(mut queue) = world.get_resource_mut::<BlockPacketQueue>() else {
            return;
        };
        queue.0.drain(..).collect()
    };

    let _span = tracing::info_span!("apply_block_actions", count = actions.len()).entered();
    let mut breaking = Vec::new();
    let mut waiting = Vec::new();
    for (entity, action) in actions {
        if breaking.contains(&entity) {
            if waiting
                .iter()
                .filter(|(waiter, _)| *waiter == entity)
                .count()
                < MAX_WAITING_BLOCK_ACTIONS
            {
                waiting.push((entity, action));
            } else {
                debug!(player = ?entity, "Dropped a block action over the waiting limit");
            }
            continue;
        }
        match action {
            BlockAction::AuthInputActions(block_actions) => {
                if process_auth_input_block_actions(world, entity, &block_actions) {
                    breaking.push(entity);
                }
            }
            BlockAction::BlockClick(use_item) => {
                handle_block_click(world, entity, &use_item);
            }
        }
    }
    if !waiting.is_empty()
        && let Some(mut queue) = world.get_resource_mut::<BlockPacketQueue>()
    {
        queue.0.splice(0..0, waiting);
    }
}

/// One PlayerAuthInput's block actions, then one input of progress for the
/// player's break (survival breaking as BDS 1.26.51 does it; see
/// [`advance_breaking`]). Whether that input completed the break.
fn process_auth_input_block_actions(
    world: &mut World,
    player_entity: Entity,
    block_actions: &[jolyne::valentine::PlayerBlockActionData],
) -> bool {
    breaking_hit_sound(world, player_entity);
    let mut predicted = None;
    // A creative start breaks at once; a prediction in the same input is
    // the client's own prediction of that break.
    let mut creative_broken = None;
    let mut creative_predicted = false;
    for action_item in block_actions.iter().take(MAX_BLOCK_ACTIONS) {
        let position = (
            action_item.position.x,
            action_item.position.y,
            action_item.position.z,
        );
        let face = action_item.facing;

        trace!(action = ?action_item.player_action_type, pos = ?position, "Block action received");
        // The face picks the crack particle event (3603 + face) and the
        // blocks a refused prediction resends; abort data is not a face.
        if matches!(
            action_item.player_action_type,
            EnumsPlayerActionType::Startdestroyblock
                | EnumsPlayerActionType::Continuedestroyblock
                | EnumsPlayerActionType::Crackblock
                | EnumsPlayerActionType::Predictdestroyblock
        ) && !(0..=5).contains(&face)
        {
            debug!(pos = ?position, face, action = ?action_item.player_action_type, "Ignored a block action with no valid face");
            continue;
        }
        if !can_interact(world, player_entity, position)
            && !matches!(
                action_item.player_action_type,
                EnumsPlayerActionType::Abortdestroyblock | EnumsPlayerActionType::Stopdestroyblock
            )
        {
            correct_block(world, player_entity, position);
            continue;
        }

        let creative = world
            .get::<crate::entity::components::GameMode>(player_entity)
            .is_some_and(|mode| mode.instant_break());
        match action_item.player_action_type {
            // No client gets here with it: BDS reads an input carrying one
            // as malformed, and `packets.rs` refuses the whole input so.
            EnumsPlayerActionType::Creativedestroyblock => {
                break_block(world, player_entity, position, true);
            }
            EnumsPlayerActionType::Predictdestroyblock if creative => {
                if creative_broken == Some(position) {
                    creative_predicted = true;
                } else {
                    refuse_prediction(world, player_entity, position, face);
                }
            }
            // Judged with this input's progress, after the actions.
            EnumsPlayerActionType::Predictdestroyblock => predicted = Some((position, face)),
            EnumsPlayerActionType::Startdestroyblock => {
                if let Some(mut event_buffer) = world.get_resource_mut::<EventBuffer>() {
                    event_buffer.push(ServerEvent::PlayerStartBreak {
                        entity: player_entity,
                        position,
                        face: 0,
                    });
                }
                if !creative {
                    start_breaking(world, player_entity, position, face);
                } else if breaks_in_creative(world, player_entity)
                    && break_block(world, player_entity, position, true)
                {
                    creative_broken = Some(position);
                }
            }
            EnumsPlayerActionType::Crackblock | EnumsPlayerActionType::Continuedestroyblock
                if !creative =>
            {
                continue_breaking(world, player_entity, position, face);
            }
            EnumsPlayerActionType::Abortdestroyblock => {
                debug!(pos = ?position, "Break aborted");
                broadcast_level_event(
                    world,
                    position,
                    block_corner(position),
                    LEVEL_EVENT_BLOCK_STOP_BREAK,
                    0,
                );
                if let Some(mut breaking) = world.get_mut::<BreakingState>(player_entity) {
                    breaking.stop();
                }
            }
            // BDS stops the crack at the position the action carries (the
            // default, the origin, from a client) and the break goes on.
            EnumsPlayerActionType::Stopdestroyblock => broadcast_level_event(
                world,
                position,
                block_corner(position),
                LEVEL_EVENT_BLOCK_STOP_BREAK,
                0,
            ),
            _ => {}
        }
    }
    // An unpredicted creative break is the server's own, as in survival.
    if let (Some(position), false) = (creative_broken, creative_predicted)
        && let Some(mut state) = world.get_mut::<BreakingState>(player_entity)
    {
        state.completing = Some(position);
    }
    advance_breaking(world, player_entity, predicted)
}

/// Items with which a creative player breaks nothing: the start breaks
/// nothing and a prediction is refused (BDS; spears and every other item
/// tried break, `blocks/testdata/bds-creative-break-items.json`).
const NO_CREATIVE_BREAK: &[&str] = &[
    "minecraft:wooden_sword",
    "minecraft:stone_sword",
    "minecraft:copper_sword",
    "minecraft:iron_sword",
    "minecraft:golden_sword",
    "minecraft:diamond_sword",
    "minecraft:netherite_sword",
    "minecraft:trident",
    "minecraft:mace",
];

/// Whether a creative player holding `item` breaks blocks.
pub(crate) fn item_breaks_in_creative(item: &str) -> bool {
    !NO_CREATIVE_BREAK.contains(&item)
}

fn breaks_in_creative(world: &World, player: Entity) -> bool {
    held_item(world, player).is_none_or(|held| item_breaks_in_creative(held.item_id()))
}

/// Before an input's block actions: on the first input after a break
/// started and every fifth after it, the other players near the block hear
/// the break's hit sound, named for the block the break started on, until
/// an input finds no block there that can break (BDS; the breaking client
/// plays its own).
fn breaking_hit_sound(world: &mut World, player: Entity) {
    let Some(mut state) = world
        .get_mut::<BreakingState>(player)
        .filter(|state| state.is_breaking())
    else {
        return;
    };
    state.ticks += 1;
    if state.ticks % 5 != 1 || !state.sounding {
        return;
    }
    let (position, block) = (state.position.expect("breaking"), state.block);
    let (cx, cz) = world_to_chunk_coords(position.0, position.2);
    let Some(viewers) = world
        .get_resource::<ChunkManager>()
        .and_then(|chunks| chunks.get_by_coords(cx, cz))
        .and_then(|chunk| world.get::<ChunkViewers>(chunk))
    else {
        return;
    };
    let packet = jolyne::valentine::LevelSoundEventPacket {
        sound_event: "hit".into(),
        position: block_centre(position),
        data: super::types::block_network_ids(world).to_wire(block) as i32,
        actor_identifier: "".into(),
        is_baby: false,
        is_global: false,
        actor_unique_id: -1,
        fire_at_position: None,
    };
    for viewer in viewers.iter().filter(|viewer| *viewer != player) {
        if let Some(session) = world.get::<PlayerSession>(viewer) {
            let _ = session.send(McpePacket::from(packet.clone()));
        }
    }
}

/// Fraction of the block at `position` one input of `player`'s breaking
/// destroys with the held item; 0 for air and unbreakable blocks.
fn break_speed(world: &World, player: Entity, position: (i32, i32, i32)) -> f64 {
    let Some(block) = block_at(world, position)
        .filter(|id| *id != *blocks::AIR)
        .and_then(|id| {
            world
                .get_resource::<super::types::BlockRegistryResource>()?
                .0
                .get_by_runtime_id(id)
        })
    else {
        return 0.0;
    };
    let tool = held_item(world, player);
    super::mining::destroy_progress(block, tool.as_ref(), mining_conditions(world, player))
}

/// The crack speed the client animates with: 65535 times the fraction one
/// input destroys, whole (BDS: dirt by hand 4369, Haste II 8807).
fn crack_data(speed: f64) -> i32 {
    (65535.0 * speed).floor().clamp(0.0, 65535.0) as i32
}

/// `StartDestroyBlock`: the crack starts at the held item's speed (0 on air);
/// the progress begins again unless the same block is being broken there
/// (BDS; see [`BreakingState::start`]).
fn start_breaking(world: &mut World, player: Entity, position: (i32, i32, i32), face: i32) {
    let speed = break_speed(world, player, position);
    let block = block_at(world, position).unwrap_or(*blocks::AIR);
    broadcast_level_event(
        world,
        position,
        block_corner(position),
        LEVEL_EVENT_BLOCK_START_BREAK,
        crack_data(speed),
    );
    if let Some(mut breaking) = world.get_mut::<BreakingState>(player) {
        breaking.start(position, block, face, speed);
    }
}

/// `ContinueDestroyBlock` (or `CrackBlock`): the crack restarts (stop, then
/// start) at the current speed, 0 on air; another position starts from no
/// progress (BDS).
fn continue_breaking(world: &mut World, player: Entity, position: (i32, i32, i32), face: i32) {
    let speed = break_speed(world, player, position);
    let block = block_at(world, position).unwrap_or(*blocks::AIR);
    let corner = block_corner(position);
    broadcast_level_event(
        world,
        position,
        corner.clone(),
        LEVEL_EVENT_BLOCK_STOP_BREAK,
        0,
    );
    broadcast_level_event(
        world,
        position,
        corner,
        LEVEL_EVENT_BLOCK_START_BREAK,
        crack_data(speed),
    );
    if let Some(mut breaking) = world.get_mut::<BreakingState>(player) {
        breaking.continue_at(position, block, face, speed);
    }
}

/// One input of progress, after the input's block actions. A break that
/// completes is the server's own (BDS breaks the block itself) unless the
/// client predicted it with this input; a prediction before the progress
/// completes is refused and the break goes on. An unfinished break shows its
/// crack particles on the worked face and its crack speed. A completed break
/// goes on at that position from zero, and while no block that can break is
/// there the progress keeps adding up silently, so a block put back breaks
/// when it completes, at once if it already has (BDS). Whether a break was
/// queued.
fn advance_breaking(
    world: &mut World,
    player: Entity,
    predicted: Option<((i32, i32, i32), i32)>,
) -> bool {
    let Some(mut state) = world
        .get::<BreakingState>(player)
        .filter(|state| state.is_breaking())
        .cloned()
    else {
        if let Some((position, face)) = predicted {
            refuse_prediction(world, player, position, face);
        }
        return false;
    };
    // A survival break does not go on in creative (BDS: a block put back
    // after the switch is not broken).
    if world
        .get::<crate::entity::components::GameMode>(player)
        .is_some_and(|mode| mode.instant_break())
    {
        if let Some(mut live) = world.get_mut::<BreakingState>(player) {
            live.stop();
        }
        return false;
    }
    let position = state.position.expect("breaking");
    if let Some((other, face)) = predicted.filter(|(at, _)| *at != position) {
        refuse_prediction(world, player, other, face);
    }
    let predicted_here = predicted.is_some_and(|(at, _)| at == position);
    state.advance();
    let block = block_at(world, position).filter(|_| break_speed(world, player, position) > 0.0);
    state.sounding &= block.is_some();
    let mut queued = false;
    match block {
        None => {}
        Some(_) if state.is_complete() => {
            state.progress = 0.0;
            queued = break_block(world, player, position, false);
            if queued && !predicted_here {
                state.completing = Some(position);
            }
        }
        Some(block_id) => {
            let wire = super::types::block_network_ids(world).to_wire(block_id) as i32;
            broadcast_level_event(
                world,
                position,
                block_centre(position),
                LEVEL_EVENT_PARTICLES_CRACK_BLOCK + state.face,
                wire,
            );
            broadcast_level_event(
                world,
                position,
                block_corner(position),
                LEVEL_EVENT_BLOCK_BREAK_SPEED,
                crack_data(state.speed),
            );
            if predicted_here {
                refuse_prediction(world, player, position, state.face);
            }
        }
    }
    if let Some(mut live) = world.get_mut::<BreakingState>(player) {
        *live = state;
    }
    queued
}

/// A `PredictDestroyBlock` before the break completed: the client gets back
/// the block, the block above it and the two on the worked face's side (BDS).
fn refuse_prediction(world: &World, player: Entity, position: (i32, i32, i32), face: i32) {
    let (x, y, z) = position;
    let side = match face {
        0 => (x, y - 1, z),
        1 => (x, y + 1, z),
        2 => (x, y, z - 1),
        3 => (x, y, z + 1),
        4 => (x - 1, y, z),
        5 => (x + 1, y, z),
        _ => position,
    };
    debug!(pos = ?position, "Refused an early break prediction");
    for at in [position, (x, y + 1, z), side, (side.0, side.1 + 1, side.2)] {
        correct_block(world, player, at);
    }
}

fn block_corner((x, y, z): (i32, i32, i32)) -> Vec3 {
    Vec3 {
        x: x as f32,
        y: y as f32,
        z: z as f32,
    }
}

fn block_centre((x, y, z): (i32, i32, i32)) -> Vec3 {
    Vec3 {
        x: x as f32 + 0.5,
        y: y as f32 + 0.5,
        z: z as f32 + 0.5,
    }
}

/// A `LevelEvent` to the viewers of the chunk holding `block`.
fn broadcast_level_event(
    world: &World,
    block: (i32, i32, i32),
    position: Vec3,
    event_id: i32,
    data: i32,
) {
    let (cx, cz) = world_to_chunk_coords(block.0, block.2);
    let Some(chunk_entity) = world
        .get_resource::<ChunkManager>()
        .and_then(|chunks| chunks.get_by_coords(cx, cz))
    else {
        return;
    };
    let packet = LevelEventPacket {
        event_id,
        position,
        data,
    };
    if let Some(chunk_viewers) = world.get::<ChunkViewers>(chunk_entity) {
        for viewer_entity in chunk_viewers.iter() {
            if let Some(session) = world.get::<PlayerSession>(viewer_entity) {
                let _ = session.send(McpePacket::from(packet.clone()));
            }
        }
    }
}

/// Runtime id of the block at `position`; `None` outside the world or an unloaded chunk.
pub(crate) fn block_at(world: &World, position: (i32, i32, i32)) -> Option<u32> {
    let (x, y, z) = position;
    if !(crate::world::chunk::MIN_Y
        ..crate::world::chunk::MIN_Y + i32::from(crate::world::chunk::SUBCHUNK_COUNT) * 16)
        .contains(&y)
    {
        return None;
    }
    let (cx, cz) = world_to_chunk_coords(x, z);
    let chunk = world
        .get_resource::<ChunkManager>()?
        .get_by_coords(cx, cz)?;
    let (x, y, z) = world_to_local_coords(x, y, z);
    Some(
        world
            .get::<crate::world::ecs::ChunkData>(chunk)?
            .inner
            .get_block(x, y, z),
    )
}

/// Reach is measured from server-tracked position to the target block's box.
/// Missing/non-finite player state and non-building modes fail closed.
fn can_interact(world: &World, entity: Entity, position: (i32, i32, i32)) -> bool {
    use crate::entity::components::{GameMode, Position};
    let Some(mode) = world.get::<GameMode>(entity) else {
        return false;
    };
    let Some(origin) = world.get::<Position>(entity).map(|position| position.0) else {
        return false;
    };
    if !mode.can_break_blocks() || !origin.is_finite() || block_at(world, position).is_none() {
        return false;
    }
    let min = glam::DVec3::new(
        f64::from(position.0),
        f64::from(position.1),
        f64::from(position.2),
    );
    let nearest = origin.clamp(min, min + glam::DVec3::ONE);
    let reach = if *mode == GameMode::Creative {
        6.0
    } else {
        5.0
    };
    origin.distance_squared(nearest) <= reach * reach
}

/// What `use` does on an interactive block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BlockUse {
    Workbench,
    Container,
}

/// An interactive block's `use` (crafting table, container) runs before item
/// placement unless the player is sneaking with an item in hand.
fn block_use(world: &World, entity: Entity, clicked: (i32, i32, i32)) -> Option<BlockUse> {
    use crate::entity::components::{HeldSlot, MainInventory, PlayerState};
    let block_use = block_at(world, clicked).and_then(|runtime_id| {
        let entry = world
            .get_resource::<super::types::BlockRegistryResource>()?
            .0
            .get_by_runtime_id(runtime_id)?;
        if entry.string_id == "minecraft:crafting_table" {
            Some(BlockUse::Workbench)
        } else {
            crate::block_entity::ContainerKind::from_block(&entry.string_id)
                .map(|_| BlockUse::Container)
        }
    })?;
    let sneaking = world
        .get::<PlayerState>(entity)
        .is_some_and(|state| state.sneaking);
    let holding = world
        .get::<HeldSlot>(entity)
        .zip(world.get::<MainInventory>(entity))
        .and_then(|(slot, main)| main.0.item(usize::from(slot.0)))
        .is_some_and(|stack| !stack.is_empty());
    (!(sneaking && holding)).then_some(block_use)
}

/// Resolve placeable content exclusively from the server's selected stack.
fn held_block(world: &World, entity: Entity) -> Option<(u32, &crate::item::ItemStack, i32)> {
    use crate::entity::components::{HeldSlot, MainInventory};
    let slot = world.get::<HeldSlot>(entity)?.0;
    if slot > 8 {
        return None;
    }
    let stack = world
        .get::<MainInventory>(entity)?
        .0
        .item(usize::from(slot))?;
    if stack.is_empty() {
        return None;
    }
    let item = stack.item()?;
    if stack.count > item.max_stack_size() {
        return None;
    }
    let block = world
        .get_resource::<super::types::BlockRegistryResource>()?
        .0
        .get_by_name(stack.item_id())?;
    Some((block.default_state_id, stack, item.network_id()))
}

fn correct_block(world: &World, entity: Entity, position: (i32, i32, i32)) {
    let Some(block_runtime_id) = block_at(world, position) else {
        return;
    };
    if let Some(session) = world.get::<PlayerSession>(entity) {
        let _ = session.send(McpePacket::from(jolyne::valentine::UpdateBlockPacket {
            block_position: BlockPos {
                x: position.0,
                y: position.1,
                z: position.2,
            },
            block_runtime_id: super::types::block_network_ids(world).to_wire(block_runtime_id),
            flags: BLOCK_UPDATE_NEIGHBOURS | BLOCK_UPDATE_NETWORK,
            layer: 0,
        }));
    }
}

/// Called only after the plugin manager has accepted this queued intent.
/// Revalidation closes races between packet processing and plugin dispatch.
pub(crate) fn commit_block_event(world: &mut World, event: &ServerEvent) {
    match event {
        ServerEvent::BlockBreak {
            entity,
            position,
            block_id,
            ..
        } => {
            if can_interact(world, *entity, *position)
                && block_at(world, *position) == Some(*block_id)
            {
                apply_break_block(world, *entity, position.0, position.1, position.2);
            } else {
                reject_block_event(world, event);
            }
        }
        ServerEvent::BlockPlace {
            entity,
            position,
            block_id,
            ..
        } => {
            let upper = if can_interact(world, *entity, *position) {
                placement_upper(world, *entity, *position, *block_id)
            } else {
                None
            };
            let Some(upper) = upper else {
                reject_block_event(world, event);
                return;
            };
            place_block(world, position.0, position.1, position.2, *block_id);
            if let Some((above, upper_state)) = upper {
                place_block(world, above.x, above.y, above.z, upper_state);
            }
            super::containers::placed(world, IVec3::new(position.0, position.1, position.2));
            if world.get::<crate::entity::components::GameMode>(*entity)
                == Some(&crate::entity::components::GameMode::Survival)
            {
                let slot = usize::from(
                    world
                        .get::<crate::entity::components::HeldSlot>(*entity)
                        .unwrap()
                        .0,
                );
                let mut inventory = world
                    .get_mut::<crate::entity::components::MainInventory>(*entity)
                    .unwrap();
                let item = inventory.0.item_mut(slot).unwrap();
                *item = item.grow(-1);
            }
            super::inventory::resync_inventory(world, *entity);
            correct_block(world, *entity, *position);
        }
        _ => {}
    }
}

pub(crate) fn reject_block_event(world: &mut World, event: &ServerEvent) {
    match event {
        ServerEvent::BlockBreak {
            entity, position, ..
        }
        | ServerEvent::BlockPlace {
            entity, position, ..
        } => {
            if let Some(mut breaking) = world.get_mut::<BreakingState>(*entity)
                && breaking.completing == Some(*position)
            {
                breaking.completing = None;
            }
            correct_block(world, *entity, *position);
            super::inventory::resync_inventory(world, *entity);
        }
        _ => {}
    }
}

/// Validate and queue a break intent; plugin dispatch commits it. `creative`
/// is a creative player's instant break; otherwise a survival break whose
/// progress completed (see [`advance_breaking`]). Whether it was queued.
fn break_block(
    world: &mut World,
    player: Entity,
    position: (i32, i32, i32),
    creative: bool,
) -> bool {
    let instant = world
        .get::<crate::entity::components::GameMode>(player)
        .is_some_and(|mode| mode.instant_break());
    if !can_interact(world, player, position) || creative != instant {
        correct_block(world, player, position);
        return false;
    }
    let Some(block_id) = block_at(world, position) else {
        return false;
    };
    if block_id == *blocks::AIR
        || (!creative && player_break_ticks(world, player, position) == u32::MAX)
    {
        correct_block(world, player, position);
        return false;
    }
    let player_id = world
        .get::<PlayerUuid>(player)
        .map(|id| id.0.to_string())
        .unwrap_or_default();
    let Some(mut events) = world.get_resource_mut::<EventBuffer>() else {
        return false;
    };
    events.push(ServerEvent::BlockBreak {
        entity: player,
        player_id,
        position,
        block_id,
    });
    true
}

/// Apply an already approved break; never called before cancellable dispatch.
fn apply_break_block(world: &mut World, breaking_player: Entity, x: i32, y: i32, z: i32) {
    debug!(pos = ?(x, y, z), "Committing block break");

    let (cx, cz) = world_to_chunk_coords(x, z);
    let (local_x, local_y, local_z) = world_to_local_coords(x, y, z);

    let chunk_entity = {
        let Some(chunk_manager) = world.get_resource::<ChunkManager>() else {
            return;
        };
        chunk_manager.get_by_coords(cx, cz)
    };

    let Some(chunk_entity) = chunk_entity else {
        debug!(chunk = ?(cx, cz), "break_block: chunk not found");
        return;
    };

    let original_block_id = world
        .get::<crate::world::ecs::ChunkData>(chunk_entity)
        .map(|cd| cd.inner.get_block(local_x, local_y, local_z))
        .unwrap_or(0);

    // A container hands its contents over before its actor goes with the block.
    let broken = super::containers::breaking(world, IVec3::new(x, y, z));

    // Set block to air
    if let Some(mut chunk_data) = world.get_mut::<crate::world::ecs::ChunkData>(chunk_entity) {
        chunk_data
            .inner
            .set_block(local_x, local_y, local_z, *blocks::AIR);
    } else {
        return;
    }

    if let Some(mut state_flags) = world.get_mut::<crate::world::ecs::ChunkStateFlags>(chunk_entity)
    {
        state_flags.mark_needs_rebroadcast();
    }

    // Trigger observer and write message
    world.trigger(BlockChanged {
        chunk_entity,
        block_pos: IVec3::new(x, y, z),
        old_block: original_block_id,
        new_block: *blocks::AIR,
    });

    world.write_message(BlockBroadcastEvent {
        chunk_entity,
        block_pos: IVec3::new(x, y, z),
        new_block: *blocks::AIR,
    });

    super::falling::schedule_gravity_checks(world, IVec3::new(x, y, z));

    // The break's particles and block updates, then the tool's wear (BDS's
    // order).
    broadcast_block_break(
        world,
        breaking_player,
        chunk_entity,
        x,
        y,
        z,
        original_block_id,
    );

    // Survival breaks drop the block's items as real item entities and wear
    // the tool.
    if original_block_id != *blocks::AIR
        && !world
            .get::<crate::entity::components::GameMode>(breaking_player)
            .is_some_and(|mode| mode.instant_break())
    {
        let tool = held_item(world, breaking_player);
        super::block_drops::drop_block_items(
            world,
            IVec3::new(x, y, z),
            original_block_id,
            tool.as_ref(),
        );
        super::block_drops::drop_block_experience(
            world,
            IVec3::new(x, y, z),
            original_block_id,
            tool.as_ref(),
        );
        wear_held_tool(world, breaking_player, original_block_id);
        super::hunger::exhaust_player(world, breaking_player, |values| values.mine);
    }
    if let Some(broken) = broken {
        super::containers::drop_contents(world, IVec3::new(x, y, z), broken);
    }
}

/// What a break shows its viewers right away: the destroy particles (whose
/// sound the client plays itself; BDS sends no sound for a player's break).
/// The new block reaches every viewer with the chunk's block updates; a
/// break the server completed itself also sends the breaking client the
/// block and the block above it at once, since it never predicted the break
/// (BDS).
fn broadcast_block_break(
    world: &mut World,
    breaking_player: Entity,
    chunk_entity: Entity,
    x: i32,
    y: i32,
    z: i32,
    original_block_id: u32,
) {
    let ids = super::types::block_network_ids(world);
    let particle_packet = LevelEventPacket {
        event_id: LEVEL_EVENT_PARTICLES_DESTROY_BLOCK,
        position: block_centre((x, y, z)),
        data: ids.to_wire(original_block_id) as i32,
    };
    let mut sent_to_breaker = false;
    if let Some(chunk_viewers) = world.get::<ChunkViewers>(chunk_entity) {
        for viewer_entity in chunk_viewers.iter() {
            if let Some(session) = world.get::<PlayerSession>(viewer_entity) {
                let _ = session.send(McpePacket::from(particle_packet.clone()));
                sent_to_breaker |= viewer_entity == breaking_player;
            }
        }
    }
    if !sent_to_breaker && let Some(session) = world.get::<PlayerSession>(breaking_player) {
        let _ = session.send(McpePacket::from(particle_packet));
    }
    let completed = world
        .get_mut::<BreakingState>(breaking_player)
        .and_then(|mut state| state.completing.take_if(|at| *at == (x, y, z)));
    if completed.is_some() {
        correct_block(world, breaking_player, (x, y, z));
        correct_block(world, breaking_player, (x, y + 1, z));
    }
}

/// The stack in the player's selected hotbar slot, if any.
fn held_item(world: &World, player: Entity) -> Option<crate::item::ItemStack> {
    let slot = world.get::<crate::entity::components::HeldSlot>(player)?.0;
    world
        .get::<crate::entity::components::MainInventory>(player)?
        .hotbar(slot)
        .filter(|stack| !stack.is_empty())
        .cloned()
}

/// Player state that affects break time, measured server-side: on ground
/// is a collision probe 1/16 block under the feet against block shapes
/// (not the client's flag), under water is the eye block being water.
fn mining_conditions(world: &World, player: Entity) -> super::mining::MiningConditions {
    use crate::entity::components::{Effects, PlayerState, Position};
    use crate::entity::systems::{BlockSource, collides_with_blocks};
    use crate::registry::collision::Aabb;
    let Some(eye) = world.get::<Position>(player).map(|p| p.0) else {
        return super::mining::MiningConditions::default();
    };
    let feet = eye - glam::DVec3::new(0.0, crate::entity::PLAYER_EYE_HEIGHT, 0.0);
    let probe = Aabb::new(
        feet + glam::DVec3::new(-0.3, -0.0625, -0.3),
        feet + glam::DVec3::new(0.3, 0.0, 0.3),
    );
    let underwater = world
        .block_at(eye.floor().as_ivec3())
        .zip(world.get_resource::<super::types::BlockRegistryResource>())
        .and_then(|(id, registry)| registry.0.get_by_runtime_id(id).map(|b| b.data))
        .is_some_and(|data| data.liquid && data.material == 5);
    super::mining::MiningConditions {
        on_ground: collides_with_blocks(world, probe),
        flying: world.get::<PlayerState>(player).is_some_and(|s| s.flying),
        riding: false,
        underwater,
        ..Default::default()
    }
    .with_effects(world.get::<Effects>(player))
}

/// Ticks `player` needs to break the block at `position` with the held item.
/// `u32::MAX` for unbreakable or unknown blocks.
fn player_break_ticks(world: &World, player: Entity, position: (i32, i32, i32)) -> u32 {
    let Some(block) = block_at(world, position).and_then(|id| {
        world
            .get_resource::<super::types::BlockRegistryResource>()?
            .0
            .get_by_runtime_id(id)
    }) else {
        return u32::MAX;
    };
    let tool = held_item(world, player);
    super::mining::break_ticks(block, tool.as_ref(), mining_conditions(world, player))
}

/// Apply mining wear to the held tool after breaking `block_runtime_id`
/// (shared durability rules; see `durability`).
fn wear_held_tool(world: &mut World, player: Entity, block_runtime_id: u32) {
    let Some(tool) = held_item(world, player) else {
        return;
    };
    let Some(block) = world
        .get_resource::<super::types::BlockRegistryResource>()
        .and_then(|r| r.0.get_by_runtime_id(block_runtime_id).cloned())
    else {
        return;
    };
    let wear = super::mining::mining_wear(&block, &tool);
    super::durability::wear_held(world, player, wear);
}

/// Handle block click from InventoryTransaction ItemUse::ClickBlock.
fn handle_block_click(
    world: &mut World,
    entity: Entity,
    data: &jolyne::valentine::types::ItemUseInventoryTransaction,
) {
    let clicked = (data.position.x, data.position.y, data.position.z);
    let offset = match data.face {
        0 => (0, -1, 0),
        1 => (0, 1, 0),
        2 => (0, 0, -1),
        3 => (0, 0, 1),
        4 => (-1, 0, 0),
        5 => (1, 0, 0),
        _ => {
            correct_block(world, entity, clicked);
            return;
        }
    };
    let Some(position) = clicked
        .0
        .checked_add(offset.0)
        .zip(clicked.1.checked_add(offset.1))
        .zip(clicked.2.checked_add(offset.2))
        .map(|((x, y), z)| (x, y, z))
    else {
        return;
    };
    if !can_interact(world, entity, clicked) {
        correct_block(world, entity, clicked);
        correct_block(world, entity, position);
        super::inventory::resync_inventory(world, entity);
        return;
    }
    // A valid placement follows this interaction immediately in EventBuffer.
    // PluginManager carries interaction cancellation to the paired placement.
    if let Some(mut events) = world.get_resource_mut::<EventBuffer>() {
        events.push(ServerEvent::PlayerInteractBlock {
            entity,
            position: clicked,
            face: data.face,
        });
    }
    match block_use(world, entity, clicked) {
        Some(BlockUse::Workbench) => {
            super::inventory::open_workbench(world, entity, clicked);
            return;
        }
        Some(BlockUse::Container) => {
            super::containers::open(world, entity, IVec3::new(clicked.0, clicked.1, clicked.2));
            return;
        }
        None => {}
    }
    // Spawn eggs spawn their mob on the clicked face instead of placing.
    let main_hand_selected = data.hand == jolyne::valentine::EnumsHandSlot::Mainhand
        && world
            .get::<crate::entity::components::HeldSlot>(entity)
            .is_some_and(|slot| data.slot == i32::from(slot.0));
    // Tools, bone meal, buckets and fire starters act on the clicked block
    // instead of placing anything.
    if main_hand_selected
        && super::item_use::on_block::use_on_block(
            world,
            entity,
            IVec3::new(clicked.0, clicked.1, clicked.2),
            data.face,
        )
    {
        return;
    }
    if main_hand_selected
        && block_at(world, clicked).is_some_and(|block| block != *blocks::AIR)
        && crate::entity::mob::interaction::interact::use_spawn_egg_on_block(
            world,
            entity,
            IVec3::new(position.0, position.1, position.2),
        )
    {
        return;
    }
    let held = held_block(world, entity);
    let block_ids = super::types::block_network_ids(world);
    let matches_held = held.as_ref().is_some_and(|(_, stack, network_id)| {
        data.hand == jolyne::valentine::EnumsHandSlot::Mainhand
            && world
                .get::<crate::entity::components::HeldSlot>(entity)
                .is_some_and(|slot| data.slot == i32::from(slot.0))
            && i32::from(data.item.id) == *network_id
            && data.item.stacksize == u16::from(stack.count)
            && u32::try_from(stack.damage).ok() == Some(data.item.auxvalue)
            && crate::item::user_data::stored_user_data(
                stack.item_id(),
                &data.item.user_data_buffer,
            ) == stack.nbt
            && u32::try_from(stack.runtime_id)
                .ok()
                .map(|id| block_ids.item_block_to_wire(i32::from(data.item.id), id))
                == Some(data.item.block_runtime_id)
    });
    let placement = held
        .as_ref()
        .filter(|_| matches_held)
        .and_then(|(held_state, _, _)| {
            let registry = &world
                .get_resource::<super::types::BlockRegistryResource>()?
                .0;
            let held_entry = registry.get_by_runtime_id(*held_state)?;
            let rotation = world
                .get::<crate::entity::components::Rotation>(entity)
                .copied()
                .unwrap_or_default();
            let input = super::placement::PlacementInput {
                clicked: IVec3::new(clicked.0, clicked.1, clicked.2),
                face: data.face,
                click: glam::DVec3::new(
                    f64::from(data.click_position.x),
                    f64::from(data.click_position.y),
                    f64::from(data.click_position.z),
                ),
                yaw: rotation.yaw,
                pitch: rotation.pitch,
            };
            super::placement::resolve_placement(world, held_entry, &input)
        });
    let Some(placement) = placement.filter(|placement| {
        let target = (
            placement.position.x,
            placement.position.y,
            placement.position.z,
        );
        can_interact(world, entity, clicked)
            && can_interact(world, entity, target)
            && block_at(world, clicked).is_some_and(|block| block != *blocks::AIR)
    }) else {
        correct_block(world, entity, clicked);
        correct_block(world, entity, position);
        super::inventory::resync_inventory(world, entity);
        return;
    };
    let target = (
        placement.position.x,
        placement.position.y,
        placement.position.z,
    );
    if placement_upper(world, entity, target, placement.state).is_none() {
        correct_block(world, entity, clicked);
        correct_block(world, entity, target);
        super::inventory::resync_inventory(world, entity);
        return;
    }
    let player_id = world
        .get::<PlayerUuid>(entity)
        .map(|id| id.0.to_string())
        .unwrap_or_default();
    if let Some(mut events) = world.get_resource_mut::<EventBuffer>() {
        events.push(ServerEvent::BlockPlace {
            entity,
            player_id,
            position: target,
            block_id: placement.state,
        });
    }
}

/// Validate placing `state` at `position` with the player's held block and
/// return the extra upper-half block (doors), `Some(None)` when there is
/// none, or `None` when the placement is not allowed (wrong item, occupied
/// target, entity in the way).
///
/// Runs both when the click is received and again at commit, after plugin
/// dispatch, so the world may have changed in between.
#[allow(clippy::option_option)]
fn placement_upper(
    world: &mut World,
    entity: Entity,
    position: (i32, i32, i32),
    state: u32,
) -> Option<Option<(IVec3, u32)>> {
    use super::placement::{double_slab, is_replaceable, obstructed, placed_boxes};
    let registry = world
        .get_resource::<super::types::BlockRegistryResource>()?
        .0
        .clone();
    let (held_state, _, _) = held_block(world, entity)?;
    let held = registry.get_by_runtime_id(held_state)?;
    let placed = registry.get_by_runtime_id(state)?;
    let pos = IVec3::new(position.0, position.1, position.2);
    let current_state = block_at(world, position)?;
    let current = registry.get_by_runtime_id(current_state)?;
    let merging_slab = double_slab(&registry, held).is_some_and(|double| {
        double.string_id == placed.string_id && current.string_id == held.string_id
    });
    let fills_free_space =
        placed.string_id == held.string_id && is_replaceable(current, current_state);
    if !(merging_slab || fills_free_space) {
        return None;
    }
    let upper = match placed.state_value(state, "upper_block_bit") {
        Some(crate::registry::block::StateValue::Byte(0)) => {
            let above = (position.0, position.1 + 1, position.2);
            let above_state = block_at(world, above)?;
            if !is_replaceable(registry.get_by_runtime_id(above_state)?, above_state) {
                return None;
            }
            let upper_state = placed.with_state_value(
                state,
                "upper_block_bit",
                crate::registry::block::StateValue::Byte(1),
            )?;
            Some((pos + IVec3::Y, upper_state))
        }
        Some(_) => return None,
        None => None,
    };
    let mut boxes = placed_boxes(&registry, pos, state);
    if let Some((above, upper_state)) = upper {
        boxes.extend(placed_boxes(&registry, above, upper_state));
    }
    if obstructed(world, &boxes) {
        return None;
    }
    Some(upper)
}

/// Place a block at world coordinates: update chunk and broadcast with the
/// place sound.
fn place_block(world: &mut World, x: i32, y: i32, z: i32, block_runtime_id: u32) {
    set_block(world, IVec3::new(x, y, z), block_runtime_id, true);
}

/// Set a block at world coordinates, broadcast the change to the chunk's
/// viewers (with the place sound when `place_sound`) and schedule gravity
/// checks for the block and the one above it.
pub(crate) fn set_block(world: &mut World, pos: IVec3, block_runtime_id: u32, place_sound: bool) {
    let (x, y, z) = (pos.x, pos.y, pos.z);
    let (cx, cz) = world_to_chunk_coords(x, z);
    let (local_x, local_y, local_z) = world_to_local_coords(x, y, z);

    let chunk_entity = {
        let Some(chunk_manager) = world.get_resource::<ChunkManager>() else {
            return;
        };
        chunk_manager.get_by_coords(cx, cz)
    };

    let Some(chunk_entity) = chunk_entity else {
        debug!(chunk = ?(cx, cz), "place_block: chunk not found");
        return;
    };

    let old_block_id = world
        .get::<crate::world::ecs::ChunkData>(chunk_entity)
        .map(|cd| cd.inner.get_block(local_x, local_y, local_z))
        .unwrap_or(0);

    // Update chunk data
    if let Some(mut chunk_data) = world.get_mut::<crate::world::ecs::ChunkData>(chunk_entity) {
        chunk_data
            .inner
            .set_block(local_x, local_y, local_z, block_runtime_id);
    } else {
        return;
    }

    if let Some(mut state_flags) = world.get_mut::<crate::world::ecs::ChunkStateFlags>(chunk_entity)
    {
        state_flags.mark_needs_rebroadcast();
    }

    world.trigger(BlockChanged {
        chunk_entity,
        block_pos: IVec3::new(x, y, z),
        old_block: old_block_id,
        new_block: block_runtime_id,
    });

    world.write_message(BlockBroadcastEvent {
        chunk_entity,
        block_pos: IVec3::new(x, y, z),
        new_block: block_runtime_id,
    });

    super::falling::schedule_gravity_checks(world, pos);

    // Broadcast to viewers
    if let Some(chunk_viewers) = world.get::<ChunkViewers>(chunk_entity) {
        use jolyne::valentine::{LevelSoundEventPacket, UpdateBlockPacket};

        let wire_block = super::types::block_network_ids(world).to_wire(block_runtime_id);
        let update_packet = UpdateBlockPacket {
            block_position: BlockPos { x, y, z },
            block_runtime_id: wire_block,
            flags: BLOCK_UPDATE_NEIGHBOURS | BLOCK_UPDATE_NETWORK,
            layer: 0,
        };

        let sound_packet = place_sound.then(|| LevelSoundEventPacket {
            sound_event: "place".into(),
            position: Vec3 {
                x: x as f32 + 0.5,
                y: y as f32 + 0.5,
                z: z as f32 + 0.5,
            },
            data: wire_block as i32,
            actor_identifier: "".into(),
            is_baby: false,
            is_global: false,
            actor_unique_id: 0,
            fire_at_position: None,
        });

        for viewer_entity in chunk_viewers.iter() {
            if let Some(session) = world.get::<PlayerSession>(viewer_entity) {
                let _ = session.send(McpePacket::from(update_packet.clone()));
                if let Some(sound) = &sound_packet {
                    let _ = session.send(McpePacket::from(sound.clone()));
                }
            }
        }
    }
}

/// Break the block at `pos` without a breaker, as `/setblock … destroy` and
/// `/fill … destroy` do: viewers see the break particles, the block becomes
/// air, and its tool-less drops and any container contents fall as items
/// (BDS's order: `LevelEvent` 2001, `UpdateBlock`, `AddItemActor`).
pub(crate) fn destroy_block(world: &mut World, pos: IVec3) {
    let Some(original) = block_at(world, (pos.x, pos.y, pos.z)) else {
        return;
    };
    if original == *blocks::AIR {
        return;
    }
    let (cx, cz) = world_to_chunk_coords(pos.x, pos.z);
    let chunk = world
        .get_resource::<ChunkManager>()
        .and_then(|chunks| chunks.get_by_coords(cx, cz));
    if let Some(viewers) = chunk.and_then(|chunk| world.get::<ChunkViewers>(chunk)) {
        let particles = LevelEventPacket {
            event_id: LEVEL_EVENT_PARTICLES_DESTROY_BLOCK,
            position: Vec3 {
                x: pos.x as f32 + 0.5,
                y: pos.y as f32 + 0.5,
                z: pos.z as f32 + 0.5,
            },
            data: super::types::block_network_ids(world).to_wire(original) as i32,
        };
        for viewer in viewers.iter() {
            if let Some(session) = world.get::<PlayerSession>(viewer) {
                let _ = session.send(McpePacket::from(particles.clone()));
            }
        }
    }
    let broken = super::containers::breaking(world, pos);
    set_block(world, pos, *blocks::AIR, false);
    super::block_drops::drop_block_items(world, pos, original, None);
    if let Some(broken) = broken {
        super::containers::drop_contents(world, pos, broken);
    }
}

#[cfg(test)]
mod breaking_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::components::{GameMode, HeldSlot, MainInventory, Position};
    use crate::world::ecs::{ChunkData, ChunkPosition};
    use glam::DVec3;

    fn block_world(mode: GameMode, position: DVec3) -> (World, Entity, Entity) {
        let mut world = World::new();
        world.insert_resource(ChunkManager::new(
            crate::world::WorldConfig::void_spawn_platform(1),
        ));
        world.insert_resource(EventBuffer::default());
        world.insert_resource(super::super::types::ServerConfigResource(Default::default()));
        world.insert_resource(super::super::types::BlockRegistryResource(
            std::sync::Arc::new(crate::registry::BlockRegistry::vanilla().clone()),
        ));
        world.insert_resource(Messages::<BlockBroadcastEvent>::default());
        let mut chunk = crate::world::chunk::Chunk::new(0, 0);
        chunk.set_block(1, 64, 1, *blocks::STONE);
        let chunk = world.spawn(ChunkData::new(chunk)).id();
        world
            .resource_mut::<ChunkManager>()
            .insert(ChunkPosition::new(0, 0), chunk);
        let player = world
            .spawn((
                mode,
                Position(position),
                MainInventory::default(),
                HeldSlot::default(),
            ))
            .id();
        (world, player, chunk)
    }

    #[test]
    fn crafting_table_use_wins_unless_sneaking_with_an_item() {
        use crate::entity::components::PlayerState;
        let (mut world, player, chunk) =
            block_world(GameMode::Survival, DVec3::new(1.5, 65.62, 1.5));
        let table = crate::registry::BlockRegistry::vanilla()
            .default_state_id_by_name("minecraft:crafting_table")
            .unwrap();
        world
            .get_mut::<ChunkData>(chunk)
            .unwrap()
            .inner
            .set_block(1, 64, 1, table);
        world.entity_mut(player).insert(PlayerState::default());
        assert_eq!(
            block_use(&world, player, (1, 64, 1)),
            Some(BlockUse::Workbench)
        );
        world.get_mut::<PlayerState>(player).unwrap().sneaking = true;
        assert!(
            block_use(&world, player, (1, 64, 1)).is_some(),
            "sneaking, empty hand"
        );
        world
            .get_mut::<MainInventory>(player)
            .unwrap()
            .0
            .set_item(0, crate::item::test_support::stack("minecraft:stone", 1))
            .unwrap();
        assert!(
            block_use(&world, player, (1, 64, 1)).is_none(),
            "sneaking with an item places"
        );
        world.get_mut::<PlayerState>(player).unwrap().sneaking = false;
        assert_eq!(
            block_use(&world, player, (1, 64, 1)),
            Some(BlockUse::Workbench)
        );
        assert!(
            block_use(&world, player, (1, 65, 1)).is_none(),
            "air is not a table"
        );
    }

    #[test]
    fn distant_break_leaves_block_unchanged() {
        let (mut world, player, chunk) =
            block_world(GameMode::Creative, DVec3::new(100.0, 64.0, 100.0));
        break_block(&mut world, player, (1, 64, 1), true);
        assert_eq!(
            world
                .get::<ChunkData>(chunk)
                .unwrap()
                .inner
                .get_block(1, 64, 1),
            *blocks::STONE
        );
    }

    #[test]
    fn adventure_break_leaves_block_unchanged() {
        let (mut world, player, chunk) =
            block_world(GameMode::Adventure, DVec3::new(1.0, 64.0, 1.0));
        break_block(&mut world, player, (1, 64, 1), false);
        assert_eq!(
            world
                .get::<ChunkData>(chunk)
                .unwrap()
                .inner
                .get_block(1, 64, 1),
            *blocks::STONE
        );
    }

    /// Only the first `MAX_BLOCK_ACTIONS` actions of one `PlayerAuthInput` are processed.
    #[test]
    fn block_actions_past_the_per_input_limit_are_ignored() {
        let (mut world, player, _) = block_world(GameMode::Creative, DVec3::new(1.0, 64.0, 1.0));
        world.init_resource::<crate::ecs::resources::TickCounter>();
        let action = jolyne::valentine::PlayerBlockActionData {
            player_action_type: EnumsPlayerActionType::Creativedestroyblock,
            position: BlockPos { x: 1, y: 64, z: 1 },
            facing: 1,
        };
        process_auth_input_block_actions(&mut world, player, &vec![action; MAX_BLOCK_ACTIONS + 6]);
        let breaks = world
            .resource_mut::<EventBuffer>()
            .drain()
            .iter()
            .filter(|event| matches!(event, ServerEvent::BlockBreak { .. }))
            .count();
        assert_eq!(breaks, MAX_BLOCK_ACTIONS);
    }

    #[test]
    fn creative_break_is_only_an_intent_until_plugins_allow_it() {
        let (mut world, player, chunk) =
            block_world(GameMode::Creative, DVec3::new(1.0, 64.0, 1.0));
        break_block(&mut world, player, (1, 64, 1), true);
        assert_eq!(
            world
                .get::<ChunkData>(chunk)
                .unwrap()
                .inner
                .get_block(1, 64, 1),
            *blocks::STONE
        );
        assert!(matches!(
            world.resource_mut::<EventBuffer>().drain().as_slice(),
            [ServerEvent::BlockBreak { .. }]
        ));
    }

    fn placement(world: &mut World) -> jolyne::valentine::ItemUseInventoryTransaction {
        use crate::registry::{BlockRegistry, ItemRegistry};
        use std::sync::Arc;
        let mut items = ItemRegistry::new();
        items.load_vanilla();
        let id = items.get_by_name("minecraft:stone").unwrap().network_id();
        world.insert_resource(super::super::types::ItemRegistryResource(Arc::new(items)));
        world.insert_resource(super::super::types::BlockRegistryResource(Arc::new(
            BlockRegistry::vanilla().clone(),
        )));
        jolyne::valentine::ItemUseInventoryTransaction {
            position: BlockPos { x: 1, y: 64, z: 1 },
            face: 1,
            slot: 0,
            item: jolyne::valentine::CerealizerNetworkItemStackDescriptorSerializedData {
                id: i16::try_from(id).unwrap(),
                stacksize: 2,
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[test]
    fn placement_without_owned_item_is_rejected() {
        let (mut world, player, chunk) =
            block_world(GameMode::Survival, DVec3::new(1.0, 64.0, 1.0));
        let data = placement(&mut world);
        handle_block_click(&mut world, player, &data);
        assert_eq!(
            world
                .get::<ChunkData>(chunk)
                .unwrap()
                .inner
                .get_block(1, 65, 1),
            *blocks::AIR
        );
        assert!(
            !world
                .resource_mut::<EventBuffer>()
                .drain()
                .iter()
                .any(|event| matches!(event, ServerEvent::BlockPlace { .. }))
        );
    }

    #[test]
    fn placement_waits_for_plugins_before_world_or_inventory_changes() {
        let (mut world, player, chunk) =
            block_world(GameMode::Survival, DVec3::new(1.0, 64.0, 1.0));
        let data = placement(&mut world);
        world
            .get_mut::<MainInventory>(player)
            .unwrap()
            .0
            .set_item(0, crate::item::test_support::stack("minecraft:stone", 2))
            .unwrap();
        handle_block_click(&mut world, player, &data);
        assert_eq!(
            world
                .get::<ChunkData>(chunk)
                .unwrap()
                .inner
                .get_block(1, 65, 1),
            *blocks::AIR
        );
        assert_eq!(
            world
                .get::<MainInventory>(player)
                .unwrap()
                .0
                .item(0)
                .unwrap()
                .count,
            2
        );
        assert!(
            world
                .resource_mut::<EventBuffer>()
                .drain()
                .iter()
                .any(|event| matches!(event, ServerEvent::BlockPlace { .. }))
        );
    }

    #[test]
    fn placing_inside_the_player_is_rejected() {
        let (mut world, player, chunk) = block_world(
            GameMode::Survival,
            DVec3::new(1.5, 65.0 + crate::entity::PLAYER_EYE_HEIGHT, 1.5),
        );
        world
            .entity_mut(player)
            .insert(crate::entity::components::Player);
        let data = placement(&mut world);
        world
            .get_mut::<MainInventory>(player)
            .unwrap()
            .0
            .set_item(0, crate::item::test_support::stack("minecraft:stone", 2))
            .unwrap();
        handle_block_click(&mut world, player, &data);
        assert!(
            !world
                .resource_mut::<EventBuffer>()
                .drain()
                .iter()
                .any(|event| matches!(event, ServerEvent::BlockPlace { .. }))
        );
        assert_eq!(
            world
                .get::<ChunkData>(chunk)
                .unwrap()
                .inner
                .get_block(1, 65, 1),
            *blocks::AIR
        );
    }

    #[test]
    fn doors_place_both_halves_facing_the_look_direction() {
        use crate::registry::block::StateValue;
        let (mut world, player, chunk) =
            block_world(GameMode::Survival, DVec3::new(3.5, 64.0, 1.5));
        let mut data = placement(&mut world);
        let door = crate::registry::BlockRegistry::vanilla()
            .get_by_name("minecraft:wooden_door")
            .unwrap();
        let items = world
            .resource::<super::super::types::ItemRegistryResource>()
            .0
            .clone();
        let door_item = items.get_by_name("minecraft:wooden_door").unwrap();
        let stack = crate::item::test_support::stack("minecraft:wooden_door", 1)
            .with_runtime_id(door.default_state_id as i32);
        data.item.id = door_item.network_id() as i16;
        data.item.stacksize = 1;
        data.item.block_runtime_id =
            crate::registry::block::BlockNetworkIds::Hashed.to_wire(door.default_state_id);
        world
            .get_mut::<MainInventory>(player)
            .unwrap()
            .0
            .set_item(0, stack)
            .unwrap();
        world
            .entity_mut(player)
            .insert(crate::entity::components::Rotation::new(270.0, 0.0));
        handle_block_click(&mut world, player, &data);
        let event = world.resource_mut::<EventBuffer>().drain().pop().unwrap();
        commit_block_event(&mut world, &event);
        let chunk = &world.get::<ChunkData>(chunk).unwrap().inner;
        let lower = chunk.get_block(1, 65, 1);
        let upper = chunk.get_block(1, 66, 1);
        assert_eq!(
            door.state_value(lower, "upper_block_bit"),
            Some(StateValue::Byte(0))
        );
        assert_eq!(
            door.state_value(upper, "upper_block_bit"),
            Some(StateValue::Byte(1))
        );
        assert_eq!(
            door.state_value(lower, "minecraft:cardinal_direction"),
            Some(StateValue::Str("east"))
        );
    }

    #[test]
    fn allowed_survival_place_consumes_exactly_one_and_cannot_replay_over_block() {
        let (mut world, player, chunk) =
            block_world(GameMode::Survival, DVec3::new(1.0, 64.0, 1.0));
        let data = placement(&mut world);
        world
            .get_mut::<MainInventory>(player)
            .unwrap()
            .0
            .set_item(0, crate::item::test_support::stack("minecraft:stone", 2))
            .unwrap();
        handle_block_click(&mut world, player, &data);
        let event = world.resource_mut::<EventBuffer>().drain().pop().unwrap();
        commit_block_event(&mut world, &event);
        assert_eq!(
            world
                .get::<ChunkData>(chunk)
                .unwrap()
                .inner
                .get_block(1, 65, 1),
            *blocks::STONE
        );
        assert_eq!(
            world
                .get::<MainInventory>(player)
                .unwrap()
                .0
                .item(0)
                .unwrap()
                .count,
            1
        );
        commit_block_event(&mut world, &event);
        assert_eq!(
            world
                .get::<MainInventory>(player)
                .unwrap()
                .0
                .item(0)
                .unwrap()
                .count,
            1
        );
    }

    #[test]
    fn deferred_place_revalidates_inventory_and_mode_before_commit() {
        for remove_item in [false, true] {
            let (mut world, player, chunk) =
                block_world(GameMode::Survival, DVec3::new(1.0, 64.0, 1.0));
            let data = placement(&mut world);
            world
                .get_mut::<MainInventory>(player)
                .unwrap()
                .0
                .set_item(0, crate::item::test_support::stack("minecraft:stone", 2))
                .unwrap();
            handle_block_click(&mut world, player, &data);
            let event = world.resource_mut::<EventBuffer>().drain().pop().unwrap();
            if remove_item {
                world
                    .get_mut::<MainInventory>(player)
                    .unwrap()
                    .0
                    .set_item(0, crate::item::ItemStack::empty())
                    .unwrap();
            } else {
                *world.get_mut::<GameMode>(player).unwrap() = GameMode::Spectator;
            }
            commit_block_event(&mut world, &event);
            assert_eq!(
                world
                    .get::<ChunkData>(chunk)
                    .unwrap()
                    .inner
                    .get_block(1, 65, 1),
                *blocks::AIR
            );
        }
    }

    fn give(world: &mut World, player: Entity, stack: crate::item::ItemStack) {
        world
            .get_mut::<MainInventory>(player)
            .unwrap()
            .0
            .set_item(0, stack)
            .unwrap();
    }

    fn floor_under(world: &mut World, chunk: Entity) {
        let mut data = world.get_mut::<ChunkData>(chunk).unwrap();
        for x in 0..16 {
            for z in 0..16 {
                data.inner.set_block(x, 62, z, *blocks::STONE);
            }
        }
    }

    /// StartBreak timing comes from the vanilla formula with the held tool
    /// and the server-measured ground state, not a flat `hardness * 100`.
    #[test]
    fn break_time_uses_tool_and_ground_state() {
        let eye = DVec3::new(1.5, 63.0 + crate::entity::PLAYER_EYE_HEIGHT, 1.5);
        let (mut world, player, chunk) = block_world(GameMode::Survival, eye);
        floor_under(&mut world, chunk);
        assert_eq!(player_break_ticks(&world, player, (1, 64, 1)), 150);
        give(
            &mut world,
            player,
            crate::item::test_support::stack("minecraft:iron_pickaxe", 1),
        );
        assert_eq!(player_break_ticks(&world, player, (1, 64, 1)), 8);
        world
            .get_mut::<crate::entity::components::Position>(player)
            .unwrap()
            .0
            .y += 1.0;
        assert_eq!(
            player_break_ticks(&world, player, (1, 64, 1)),
            38,
            "airborne mining is five times slower"
        );
    }

    #[test]
    fn breaking_wears_the_tool_and_a_spent_tool_breaks() {
        use crate::item::enchant::{durability_damage, with_durability_damage};
        let eye = DVec3::new(1.5, 63.0 + crate::entity::PLAYER_EYE_HEIGHT, 1.5);
        let (mut world, player, chunk) = block_world(GameMode::Survival, eye);
        floor_under(&mut world, chunk);
        let mut items = crate::registry::ItemRegistry::new();
        items.load_vanilla();
        world.insert_resource(super::super::types::ItemRegistryResource(
            std::sync::Arc::new(items),
        ));
        give(
            &mut world,
            player,
            crate::item::test_support::stack("minecraft:iron_pickaxe", 1),
        );
        wear_held_tool(&mut world, player, *blocks::STONE);
        let held = held_item(&world, player).unwrap();
        assert_eq!(durability_damage(&held), 1);

        give(&mut world, player, with_durability_damage(&held, 249));
        wear_held_tool(&mut world, player, *blocks::STONE);
        assert!(
            held_item(&world, player).is_none(),
            "iron pickaxe has 250 uses"
        );
    }

    /// A survival break advances once per input, idle inputs included
    /// (BDS): a prediction before the progress completes is refused and the
    /// block stays, and the server completes the break itself on the input
    /// that finishes it (dirt by hand: the 15th).
    #[test]
    fn survival_breaks_advance_per_input_and_refuse_early_predictions() {
        let eye = DVec3::new(1.5, 63.0 + crate::entity::PLAYER_EYE_HEIGHT, 1.5);
        let (mut world, player, chunk) = block_world(GameMode::Survival, eye);
        floor_under(&mut world, chunk);
        world
            .get_mut::<ChunkData>(chunk)
            .unwrap()
            .inner
            .set_block(1, 64, 1, *blocks::DIRT);
        world.entity_mut(player).insert(BreakingState::default());
        let action = |kind| jolyne::valentine::PlayerBlockActionData {
            player_action_type: kind,
            position: BlockPos { x: 1, y: 64, z: 1 },
            facing: 1,
        };
        let breaks = |world: &mut World| {
            world
                .resource_mut::<EventBuffer>()
                .drain()
                .into_iter()
                .filter(|event| matches!(event, ServerEvent::BlockBreak { .. }))
                .collect::<Vec<_>>()
        };
        process_auth_input_block_actions(
            &mut world,
            player,
            &[action(EnumsPlayerActionType::Startdestroyblock)],
        );
        for _ in 0..6 {
            process_auth_input_block_actions(
                &mut world,
                player,
                &[action(EnumsPlayerActionType::Continuedestroyblock)],
            );
        }
        process_auth_input_block_actions(
            &mut world,
            player,
            &[action(EnumsPlayerActionType::Predictdestroyblock)],
        );
        assert!(breaks(&mut world).is_empty(), "the 8th input is early");
        let state = world.get::<BreakingState>(player).unwrap().clone();
        assert_eq!(state.position, Some((1, 64, 1)), "the break goes on");
        assert!((state.progress - 8.0 / 15.0).abs() < 1e-9);
        for _ in 0..6 {
            process_auth_input_block_actions(&mut world, player, &[]);
        }
        assert!(breaks(&mut world).is_empty(), "14 inputs are not enough");
        process_auth_input_block_actions(&mut world, player, &[]);
        let event = breaks(&mut world)
            .pop()
            .expect("the server completes the break");
        let state = world.get::<BreakingState>(player).unwrap();
        assert_eq!(
            (state.position, state.progress),
            (Some((1, 64, 1)), 0.0),
            "BDS goes on breaking that position from zero"
        );
        commit_block_event(&mut world, &event);
        assert_eq!(
            world
                .get::<ChunkData>(chunk)
                .unwrap()
                .inner
                .get_block(1, 64, 1),
            *blocks::AIR
        );
    }

    /// The face picks a LevelEvent (3603 + face), so a breaking action with
    /// a face outside 0..=5 is ignored rather than turned into another event
    /// (or an overflow).
    #[test]
    fn breaking_actions_without_a_valid_face_are_ignored() {
        let eye = DVec3::new(1.5, 63.0 + crate::entity::PLAYER_EYE_HEIGHT, 1.5);
        let (mut world, player, chunk) = block_world(GameMode::Survival, eye);
        floor_under(&mut world, chunk);
        world
            .get_mut::<ChunkData>(chunk)
            .unwrap()
            .inner
            .set_block(1, 64, 1, *blocks::DIRT);
        world.entity_mut(player).insert(BreakingState::default());
        for (kind, facing) in [
            (EnumsPlayerActionType::Startdestroyblock, -602),
            (EnumsPlayerActionType::Continuedestroyblock, 6),
            (EnumsPlayerActionType::Predictdestroyblock, i32::MAX),
        ] {
            process_auth_input_block_actions(
                &mut world,
                player,
                &[jolyne::valentine::PlayerBlockActionData {
                    player_action_type: kind,
                    position: BlockPos { x: 1, y: 64, z: 1 },
                    facing,
                }],
            );
        }
        assert!(!world.get::<BreakingState>(player).unwrap().is_breaking());
        assert!(
            world
                .resource_mut::<EventBuffer>()
                .drain()
                .iter()
                .all(|event| !matches!(event, ServerEvent::BlockBreak { .. }))
        );
    }

    /// A survival client cannot skip its break with a creative destroy.
    #[test]
    fn survival_creative_destroy_is_refused() {
        let eye = DVec3::new(1.5, 63.0 + crate::entity::PLAYER_EYE_HEIGHT, 1.5);
        let (mut world, player, chunk) = block_world(GameMode::Survival, eye);
        floor_under(&mut world, chunk);
        world
            .get_mut::<ChunkData>(chunk)
            .unwrap()
            .inner
            .set_block(1, 64, 1, *blocks::DIRT);
        world.entity_mut(player).insert(BreakingState::default());
        process_auth_input_block_actions(
            &mut world,
            player,
            &[jolyne::valentine::PlayerBlockActionData {
                player_action_type: EnumsPlayerActionType::Creativedestroyblock,
                position: BlockPos { x: 1, y: 64, z: 1 },
                facing: 1,
            }],
        );
        assert!(
            world
                .resource_mut::<EventBuffer>()
                .drain()
                .iter()
                .all(|event| !matches!(event, ServerEvent::BlockBreak { .. }))
        );
        assert_eq!(
            world
                .get::<ChunkData>(chunk)
                .unwrap()
                .inner
                .get_block(1, 64, 1),
            *blocks::DIRT
        );
    }

    #[test]
    fn forged_placement_sends_authoritative_block_and_inventory_corrections() {
        use jolyne::valentine::McpePacketData;
        let (mut world, player, _) = block_world(GameMode::Survival, DVec3::new(1.0, 64.0, 1.0));
        let data = placement(&mut world);
        let (tx, mut rx) = tokio::sync::mpsc::channel(8);
        world
            .entity_mut(player)
            .insert(PlayerSession::new(1, "test".into(), None, None, tx));
        handle_block_click(&mut world, player, &data);
        let McpePacketData::UpdateBlockPacket(clicked) = rx.try_recv().unwrap().data else {
            panic!("clicked block correction")
        };
        assert_eq!(clicked.block_position, data.position);
        let ids = crate::registry::block::BlockNetworkIds::Hashed;
        assert_eq!(clicked.block_runtime_id, ids.to_wire(*blocks::STONE));
        let McpePacketData::UpdateBlockPacket(placed) = rx.try_recv().unwrap().data else {
            panic!("predicted block correction")
        };
        assert_eq!(placed.block_position, BlockPos { x: 1, y: 65, z: 1 });
        assert_eq!(placed.block_runtime_id, ids.to_wire(*blocks::AIR));
        let McpePacketData::InventoryContentPacket(inventory) = rx.try_recv().unwrap().data else {
            panic!("inventory correction")
        };
        assert!(
            inventory
                .slots
                .iter()
                .all(|slot| slot.id == 0 && slot.stacksize == 0)
        );
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn plugin_given_block_echo_can_be_placed_and_consumed() {
        use crate::ecs::events::{ActionQueue, PluginAction};
        use crate::entity::components::{Rotation, RuntimeEntityId};
        let (mut world, player, chunk) =
            block_world(GameMode::Survival, DVec3::new(1.0, 64.0, 1.0));
        let mut data = placement(&mut world);
        let (tx, mut rx) = tokio::sync::mpsc::channel(16);
        world.entity_mut(player).insert((
            Rotation::default(),
            RuntimeEntityId(1),
            PlayerSession::new(1, "test".into(), None, None, tx),
        ));
        let mut actions = ActionQueue::default();
        actions.push(PluginAction::GiveItem {
            entity: player,
            item_id: "minecraft:stone".into(),
            count: 2,
        });
        world.insert_resource(actions);
        let mut schedule = Schedule::default();
        schedule.add_systems(super::super::plugins::process_plugin_actions);
        schedule.run(&mut world);
        let jolyne::valentine::McpePacketData::InventorySlotPacket(slot) =
            rx.try_recv().unwrap().data
        else {
            panic!("given slot")
        };
        data.item = slot.item;
        handle_block_click(&mut world, player, &data);
        let event = world.resource_mut::<EventBuffer>().drain().pop().unwrap();
        commit_block_event(&mut world, &event);
        assert_eq!(
            world
                .get::<ChunkData>(chunk)
                .unwrap()
                .inner
                .get_block(1, 65, 1),
            *blocks::STONE
        );
        assert_eq!(
            world
                .get::<MainInventory>(player)
                .unwrap()
                .0
                .item(0)
                .unwrap()
                .count,
            1
        );
    }

    /// A cow spawn egg used on the top face of grass spawns a cow standing on
    /// that block, and uses one egg in survival but none in creative.
    #[test]
    fn cow_spawn_egg_on_grass_spawns_a_cow_on_top() {
        use crate::entity::mob::MobKind;
        for (mode, eggs_left) in [(GameMode::Survival, 1), (GameMode::Creative, 2)] {
            let mut server = super::super::GameServer::with_config(super::super::ServerConfig {
                world: crate::world::WorldConfig::void_spawn_platform(1),
                ..Default::default()
            });
            let world = server.ecs.world_mut();
            world.insert_resource(crate::entity::EntityRandom::seeded(3));
            let mut column = crate::world::chunk::Chunk::new(0, 0);
            for x in 0..16 {
                for z in 0..16 {
                    column.set_block(x, 63, z, *blocks::GRASS_BLOCK);
                }
            }
            let chunk = world.spawn(ChunkData::new(column)).id();
            world
                .resource_mut::<ChunkManager>()
                .insert(ChunkPosition::new(0, 0), chunk);
            let egg = super::super::world_stack(world, "minecraft:cow_spawn_egg", 2).unwrap();
            let egg_id = world
                .resource::<super::super::types::ItemRegistryResource>()
                .0
                .get_by_name("minecraft:cow_spawn_egg")
                .unwrap()
                .network_id();
            let mut inventory = MainInventory::default();
            inventory.0.set_item(0, egg).unwrap();
            let player = world
                .spawn((
                    Position(DVec3::new(
                        5.5,
                        64.0 + crate::entity::PLAYER_EYE_HEIGHT,
                        5.5,
                    )),
                    mode,
                    inventory,
                    HeldSlot(0),
                ))
                .id();
            let click = jolyne::valentine::ItemUseInventoryTransaction {
                position: BlockPos { x: 7, y: 63, z: 8 },
                face: 1,
                slot: 0,
                item: jolyne::valentine::CerealizerNetworkItemStackDescriptorSerializedData {
                    id: i16::try_from(egg_id).unwrap(),
                    stacksize: 2,
                    ..Default::default()
                },
                ..Default::default()
            };
            world
                .resource_mut::<BlockPacketQueue>()
                .0
                .push((player, BlockAction::BlockClick(click)));
            server.tick();

            let world = server.ecs.world_mut();
            let cows: Vec<DVec3> = world
                .query_filtered::<&Position, With<MobKind>>()
                .iter(world)
                .map(|position| position.0)
                .collect();
            assert_eq!(cows.len(), 1, "{mode:?}: one cow spawned");
            assert_eq!(cows[0].x, 7.5);
            assert_eq!(cows[0].z, 8.5);
            assert!(
                (64.0..64.01).contains(&cows[0].y),
                "{mode:?}: standing on the grass, at {}",
                cows[0].y
            );
            assert_eq!(
                world
                    .get::<MainInventory>(player)
                    .unwrap()
                    .0
                    .item(0)
                    .unwrap()
                    .count,
                eggs_left,
                "{mode:?}"
            );
            assert_eq!(
                world
                    .get::<ChunkData>(chunk)
                    .unwrap()
                    .inner
                    .get_block(7, 64, 8),
                *blocks::AIR,
                "nothing placed"
            );
        }
    }
}
