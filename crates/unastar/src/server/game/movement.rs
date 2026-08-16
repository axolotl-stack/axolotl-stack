//! Movement domain system.
//!
//! Processes PlayerAuthInput and PlayerAction packets, updating position,
//! rotation, input state, and player state flags. Forwards block actions
//! from AuthInput to the block packet queue.

use bevy_ecs::prelude::*;
use glam::DVec3;
use tracing::trace;

const MAX_AUTH_INPUT_MOVE_DELTA_PER_TICK: f64 = 64.0;

use super::packet_queues::{
    BlockAction, BlockPacketQueue, MovementEvents, MovementInput, MovementPacketQueue,
};
use crate::ecs::events::ServerEvent;
use crate::entity::components::transform::{Position, Rotation};
use crate::entity::components::{PlayerInput, PlayerState};
use jolyne::valentine::types::{EnumsPlayerActionType, EnumsPlayerAuthInputPacketPayloadInputData};

/// ECS system: drain movement packet queue and apply state changes.
///
/// Runs in `PacketApplySet`. Can execute in parallel with inventory, chat, and chunks
/// because it writes disjoint components (Position, Rotation, PlayerState, PlayerInput).
pub fn apply_movement(
    mut queue: ResMut<MovementPacketQueue>,
    mut block_queue: ResMut<BlockPacketQueue>,
    mut events: ResMut<MovementEvents>,
    mut players: Query<(
        &mut Position,
        &mut Rotation,
        &mut PlayerState,
        &mut PlayerInput,
    )>,
) {
    let _span = tracing::info_span!("apply_movement", count = queue.0.len()).entered();
    for (entity, input) in queue.0.drain(..) {
        match input {
            MovementInput::AuthInput(pk) => {
                process_auth_input(entity, &pk, &mut players, &mut events);

                // Forward block actions to block queue
                if let Some(Some(block_actions)) = pk.player_block_actions
                    && !block_actions.is_empty()
                {
                    block_queue
                        .0
                        .push((entity, BlockAction::AuthInputActions(block_actions)));
                }
            }
            MovementInput::PlayerAction(pk) => {
                process_player_action(entity, &pk, &mut players, &mut events);
            }
        }
    }
}

/// Process a PlayerAuthInput packet: position, rotation, input flags, state toggles.
fn process_auth_input(
    entity: Entity,
    pk: &jolyne::valentine::PlayerAuthInputPacket,
    players: &mut Query<(
        &mut Position,
        &mut Rotation,
        &mut PlayerState,
        &mut PlayerInput,
    )>,
    events: &mut ResMut<MovementEvents>,
) {
    let Ok((mut pos, mut rot, mut state, mut input)) = players.get_mut(entity) else {
        return;
    };

    // Update position
    let new_pos = DVec3::new(
        pk.position.x as f64,
        pk.position.y as f64,
        pk.position.z as f64,
    );
    let old_pos = pos.0;
    if old_pos.distance(new_pos) > MAX_AUTH_INPUT_MOVE_DELTA_PER_TICK {
        tracing::warn!(
            entity = ?entity,
            from = ?old_pos,
            to = ?new_pos,
            "Rejected implausible PlayerAuthInput movement delta"
        );
        return;
    }
    pos.0 = new_pos;

    // Update rotation
    rot.pitch = pk.player_rotation.x;
    rot.yaw = pk.player_rotation.y;
    rot.head_yaw = pk.player_head_rotation;

    // Update input
    let input_data = pk.input_data.as_deref().unwrap_or_default();
    let was_jumping = input.jumping;
    input.move_x = pk.move_vector.x;
    input.move_z = pk.move_vector.y;
    input.jumping = input_data.contains(&EnumsPlayerAuthInputPacketPayloadInputData::Jumping)
        || input_data.contains(&EnumsPlayerAuthInputPacketPayloadInputData::StartJumping);
    let jumped = input.jumping && !was_jumping;
    input.sneaking = input_data.contains(&EnumsPlayerAuthInputPacketPayloadInputData::Sneaking);
    input.sprinting = input_data.contains(&EnumsPlayerAuthInputPacketPayloadInputData::Sprinting);
    input.tick = pk.client_tick.inputtick as i64;
    input.on_ground =
        !input_data.contains(&EnumsPlayerAuthInputPacketPayloadInputData::VerticalCollision);

    // Update persistent state flags
    let mut toggle_sneak = None;
    let mut toggle_sprint = None;

    if input_data.contains(&EnumsPlayerAuthInputPacketPayloadInputData::StartSneaking) {
        state.sneaking = true;
        toggle_sneak = Some(true);
    }
    if input_data.contains(&EnumsPlayerAuthInputPacketPayloadInputData::StopSneaking) {
        state.sneaking = false;
        toggle_sneak = Some(false);
    }
    if input_data.contains(&EnumsPlayerAuthInputPacketPayloadInputData::StartSprinting) {
        state.sprinting = true;
        toggle_sprint = Some(true);
    }
    if input_data.contains(&EnumsPlayerAuthInputPacketPayloadInputData::StopSprinting) {
        state.sprinting = false;
        toggle_sprint = Some(false);
    }
    if input_data.contains(&EnumsPlayerAuthInputPacketPayloadInputData::StartSwimming) {
        state.swimming = true;
    }
    if input_data.contains(&EnumsPlayerAuthInputPacketPayloadInputData::StopSwimming) {
        state.swimming = false;
    }
    if input_data.contains(&EnumsPlayerAuthInputPacketPayloadInputData::StartGliding) {
        state.gliding = true;
    }
    if input_data.contains(&EnumsPlayerAuthInputPacketPayloadInputData::StopGliding) {
        state.gliding = false;
    }
    if input_data.contains(&EnumsPlayerAuthInputPacketPayloadInputData::StartFlying) {
        state.flying = true;
    }
    if input_data.contains(&EnumsPlayerAuthInputPacketPayloadInputData::StopFlying) {
        state.flying = false;
    }

    // Emit events
    if old_pos.distance_squared(new_pos) > 1e-3 {
        events.0.push(ServerEvent::PlayerMove {
            entity,
            from: (old_pos.x, old_pos.y, old_pos.z),
            to: (new_pos.x, new_pos.y, new_pos.z),
        });
    }
    if jumped {
        events.0.push(ServerEvent::PlayerJump { entity });
    }
    if let Some(is_sneaking) = toggle_sneak {
        events.0.push(ServerEvent::PlayerToggleSneak {
            entity,
            is_sneaking,
        });
    }
    if let Some(is_sprinting) = toggle_sprint {
        events.0.push(ServerEvent::PlayerToggleSprint {
            entity,
            is_sprinting,
        });
    }
}

/// Process a PlayerAction packet: sprint/sneak state, StartBreak event.
fn process_player_action(
    entity: Entity,
    pk: &jolyne::valentine::PlayerActionPacket,
    players: &mut Query<(
        &mut Position,
        &mut Rotation,
        &mut PlayerState,
        &mut PlayerInput,
    )>,
    events: &mut ResMut<MovementEvents>,
) {
    trace!(action = ?pk.action, "PlayerAction received");

    match pk.action {
        EnumsPlayerActionType::StartJump => {
            trace!("Player jumped");
        }
        EnumsPlayerActionType::StartDestroyBlock => {
            events.0.push(ServerEvent::PlayerStartBreak {
                entity,
                position: (
                    pk.block_position.x,
                    pk.block_position.y,
                    pk.block_position.z,
                ),
                face: pk.face as u8,
            });
        }
        EnumsPlayerActionType::StartSprinting => {
            if let Ok((_, _, mut state, _)) = players.get_mut(entity) {
                state.sprinting = true;
            }
        }
        EnumsPlayerActionType::StopSprinting => {
            if let Ok((_, _, mut state, _)) = players.get_mut(entity) {
                state.sprinting = false;
            }
        }
        EnumsPlayerActionType::StartSneaking => {
            if let Ok((_, _, mut state, _)) = players.get_mut(entity) {
                state.sneaking = true;
            }
        }
        EnumsPlayerActionType::StopSneaking => {
            if let Ok((_, _, mut state, _)) = players.get_mut(entity) {
                state.sneaking = false;
            }
        }
        EnumsPlayerActionType::Respawn => {
            trace!("Player requested respawn");
        }
        EnumsPlayerActionType::ChangeDimensionAck => {
            trace!("Player acknowledged dimension change");
        }
        EnumsPlayerActionType::HandledTeleport => {
            trace!("Player handled teleport");
        }
        _ => {
            trace!(action = ?pk.action, "Unhandled player action");
        }
    }
}
