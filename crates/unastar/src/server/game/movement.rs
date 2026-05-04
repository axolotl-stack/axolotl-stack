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
use jolyne::valentine::types::{Action, InputFlag};

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
                if let Some(block_actions) = pk.block_action
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
    rot.pitch = pk.pitch;
    rot.yaw = pk.yaw;
    rot.head_yaw = pk.head_yaw;

    // Update input
    let was_jumping = input.jumping;
    input.move_x = pk.move_vector.x;
    input.move_z = pk.move_vector.z;
    input.jumping = pk.input_data.contains(InputFlag::JUMPING)
        || pk.input_data.contains(InputFlag::START_JUMPING);
    let jumped = input.jumping && !was_jumping;
    input.sneaking = pk.input_data.contains(InputFlag::SNEAKING);
    input.sprinting = pk.input_data.contains(InputFlag::SPRINTING);
    input.tick = pk.tick;
    input.on_ground = !pk.input_data.contains(InputFlag::VERTICAL_COLLISION);

    // Update persistent state flags
    let mut toggle_sneak = None;
    let mut toggle_sprint = None;

    if pk.input_data.contains(InputFlag::START_SNEAKING) {
        state.sneaking = true;
        toggle_sneak = Some(true);
    }
    if pk.input_data.contains(InputFlag::STOP_SNEAKING) {
        state.sneaking = false;
        toggle_sneak = Some(false);
    }
    if pk.input_data.contains(InputFlag::START_SPRINTING) {
        state.sprinting = true;
        toggle_sprint = Some(true);
    }
    if pk.input_data.contains(InputFlag::STOP_SPRINTING) {
        state.sprinting = false;
        toggle_sprint = Some(false);
    }
    if pk.input_data.contains(InputFlag::START_SWIMMING) {
        state.swimming = true;
    }
    if pk.input_data.contains(InputFlag::STOP_SWIMMING) {
        state.swimming = false;
    }
    if pk.input_data.contains(InputFlag::START_GLIDING) {
        state.gliding = true;
    }
    if pk.input_data.contains(InputFlag::STOP_GLIDING) {
        state.gliding = false;
    }
    if pk.input_data.contains(InputFlag::START_FLYING) {
        state.flying = true;
    }
    if pk.input_data.contains(InputFlag::STOP_FLYING) {
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
        Action::Jump => {
            trace!("Player jumped");
        }
        Action::StartBreak => {
            events.0.push(ServerEvent::PlayerStartBreak {
                entity,
                position: (pk.position.x, pk.position.y, pk.position.z),
                face: pk.face as u8,
            });
        }
        Action::StartSprint => {
            if let Ok((_, _, mut state, _)) = players.get_mut(entity) {
                state.sprinting = true;
            }
        }
        Action::StopSprint => {
            if let Ok((_, _, mut state, _)) = players.get_mut(entity) {
                state.sprinting = false;
            }
        }
        Action::StartSneak => {
            if let Ok((_, _, mut state, _)) = players.get_mut(entity) {
                state.sneaking = true;
            }
        }
        Action::StopSneak => {
            if let Ok((_, _, mut state, _)) = players.get_mut(entity) {
                state.sneaking = false;
            }
        }
        Action::Respawn => {
            trace!("Player requested respawn");
        }
        Action::DimensionChangeAck => {
            trace!("Player acknowledged dimension change");
        }
        Action::HandledTeleport => {
            trace!("Player handled teleport");
        }
        _ => {
            trace!(action = ?pk.action, "Unhandled player action");
        }
    }
}
