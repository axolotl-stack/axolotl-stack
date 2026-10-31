//! Movement domain system.
//!
//! Processes PlayerAuthInput and PlayerAction packets, updating position,
//! rotation, input state, player state flags and the fall the player life
//! systems resolve. Forwards block actions from AuthInput to the block
//! packet queue. (`PlayerAction` respawn is routed to
//! `player_life::respawn` instead.)

use super::player_events::{self, ConnectionIdentity, PlayerMove};
use bevy_ecs::prelude::*;
use bevy_ecs::system::SystemState;
use glam::DVec3;
use tracing::trace;

const MAX_AUTH_INPUT_MOVE_DELTA_PER_TICK: f64 = 64.0;

use super::packet_queues::{
    BlockAction, BlockPacketQueue, MovementEvents, MovementInput, MovementPacketQueue,
};
use crate::ecs::events::ServerEvent;
use crate::entity::components::transform::{Position, Rotation};
use crate::entity::components::{GameMode, Hunger, PlayerInput, PlayerState};
use crate::entity::player_life::environment::PlayerFall;
use jolyne::valentine::types::{EnumsPlayerActionType, EnumsPlayerAuthInputPacketPayloadInputData};

/// The player state the movement packets write.
type MovingPlayers<'w, 's> = Query<
    'w,
    's,
    (
        &'static mut Position,
        &'static mut Rotation,
        &'static mut PlayerState,
        &'static mut PlayerInput,
        Option<&'static mut PlayerFall>,
    ),
>;

/// Query and resource state reacquired only after native callbacks finish.
type MovementSystemState = SystemState<(
    ResMut<'static, MovementEvents>,
    Res<'static, super::hunger::ExhaustionValues>,
    MovingPlayers<'static, 'static>,
    Query<'static, 'static, (&'static mut Hunger, &'static GameMode)>,
)>;

/// Drain movement inputs, authorize proposals and commit without borrowed state
/// across native callbacks. Runs before physics in `PacketApplySet`.
pub fn apply_movement(world: &mut World, system_state: &mut MovementSystemState) {
    let inputs = std::mem::take(&mut world.resource_mut::<MovementPacketQueue>().0);
    for (entity, input) in inputs {
        match input {
            MovementInput::AuthInput(mut pk) => {
                let Some(identity) = ConnectionIdentity::capture(world, entity) else {
                    continue;
                };
                let Some(from) = world.get::<Position>(entity).map(|position| position.0) else {
                    continue;
                };
                let proposed = DVec3::new(
                    f64::from(pk.position.x),
                    f64::from(pk.position.y),
                    f64::from(pk.position.z),
                );
                if !valid_move(from, proposed)
                    || !pk.player_rotation.x.is_finite()
                    || !pk.player_rotation.y.is_finite()
                    || !pk.player_head_rotation.is_finite()
                    || !pk.move_vector.x.is_finite()
                    || !pk.move_vector.y.is_finite()
                {
                    correct_position(world, entity, from);
                    continue;
                }
                let mode = world.get::<GameMode>(entity).copied();
                let health = world
                    .get::<crate::entity::components::Health>(entity)
                    .map(|health| (health.current, health.max));
                let dead = world
                    .get::<crate::entity::player_life::Dead>(entity)
                    .is_some();
                let mut event = PlayerMove::new(entity, identity.clone(), from, proposed);
                player_events::movement(world, &mut event);
                let unchanged = identity.matches(world, entity)
                    && world.get::<GameMode>(entity).copied() == mode
                    && world
                        .get::<crate::entity::components::Health>(entity)
                        .map(|health| (health.current, health.max))
                        == health
                    && world
                        .get::<crate::entity::player_life::Dead>(entity)
                        .is_some()
                        == dead
                    && world
                        .get::<Position>(entity)
                        .is_some_and(|position| position.0 == from);
                if !unchanged {
                    if identity.matches(world, entity) {
                        let current = world
                            .get::<Position>(entity)
                            .map_or(from, |position| position.0);
                        correct_position(world, entity, current);
                    }
                    continue;
                }
                if event.is_cancelled() || !valid_move(from, event.destination) {
                    correct_position(world, entity, from);
                    continue;
                }
                // The host applies the same representable destination its client
                // will receive, so domain notifications describe committed state.
                pk.position.x = event.destination.x as f32;
                pk.position.y = event.destination.y as f32;
                pk.position.z = event.destination.z as f32;
                let committed = {
                    let (mut events, exhaustion, mut players, mut hungry) =
                        system_state.get_mut(world);
                    let moved = process_auth_input(entity, &pk, &mut players, &mut events);
                    if let (Some(moved), Ok((mut hunger, mode))) = (moved, hungry.get_mut(entity))
                        && !matches!(mode, GameMode::Creative | GameMode::Spectator)
                    {
                        hunger.exhaust(moved.exhaustion(&exhaustion));
                    }
                    moved.is_some()
                };
                if !committed {
                    if identity.matches(world, entity) {
                        correct_position(world, entity, from);
                    }
                    continue;
                }
                if let Some(block_actions) = pk.player_block_actions
                    && !block_actions.is_empty()
                {
                    world
                        .resource_mut::<BlockPacketQueue>()
                        .0
                        .push((entity, BlockAction::AuthInputActions(block_actions)));
                }
                let to = world
                    .get::<Position>(entity)
                    .map(|position| position.0)
                    .unwrap_or(from);
                if event.destination != proposed {
                    correct_position(world, entity, to);
                }
                if to != from {
                    player_events::moved(world, entity, identity, from, to);
                }
            }
            MovementInput::PlayerAction(pk) => {
                let (mut events, _, mut players, _) = system_state.get_mut(world);
                process_player_action(entity, &pk, &mut players, &mut events);
            }
        }
    }
    system_state.apply(world);
}

/// Reject nonfinite, unrepresentable or implausibly large input/redirect moves.
fn valid_move(from: DVec3, to: DVec3) -> bool {
    from.is_finite()
        && to.is_finite()
        && to.as_vec3().is_finite()
        && from.distance(to) <= MAX_AUTH_INPUT_MOVE_DELTA_PER_TICK
}

/// Send the current authoritative eye position without applying another move.
fn correct_position(world: &World, player: Entity, position: DVec3) {
    use crate::entity::components::{PlayerSession, RuntimeEntityId};
    use jolyne::valentine::{
        ActorRuntimeId, EnumsPlayerPositionModeComponentPositionMode, MovePlayerPacket, Vec2, Vec3,
    };
    let (Some(session), Some(runtime), Some(rotation)) = (
        world.get::<PlayerSession>(player),
        world.get::<RuntimeEntityId>(player),
        world.get::<Rotation>(player),
    ) else {
        return;
    };
    session.send(
        MovePlayerPacket {
            player_runtime_id: ActorRuntimeId {
                actor_runtime_id: runtime.0 as u64,
            },
            position: Vec3 {
                x: position.x as f32,
                y: position.y as f32,
                z: position.z as f32,
            },
            rotation: Vec2 {
                x: rotation.pitch,
                y: rotation.yaw,
            },
            y_head_rotation: rotation.head_yaw,
            position_mode: EnumsPlayerPositionModeComponentPositionMode::Teleport,
            teleport_data: Some(jolyne::valentine::MovePlayerTeleportData {
                teleportation_cause: 3,
                source_actor_type: 63,
            }),
            tick: crate::entity::components::input_tick_of(world, player),
            ..Default::default()
        }
        .into(),
    );
}

/// Process a PlayerAuthInput packet: position, rotation, input flags, state toggles.
fn process_auth_input(
    entity: Entity,
    pk: &jolyne::valentine::PlayerAuthInputPacket,
    players: &mut MovingPlayers,
    events: &mut ResMut<MovementEvents>,
) -> Option<Moved> {
    let Ok((mut pos, mut rot, mut state, mut input, fall)) = players.get_mut(entity) else {
        return None;
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
        return None;
    }
    pos.0 = new_pos;

    // Update rotation
    rot.pitch = pk.player_rotation.x;
    rot.yaw = pk.player_rotation.y;
    rot.head_yaw = pk.player_head_rotation;

    // Update input
    let input_data = &pk.input_data;
    let was_jumping = input.jumping;
    input.move_x = pk.move_vector.x;
    input.move_z = pk.move_vector.y;
    input.jumping = input_data.contains(&EnumsPlayerAuthInputPacketPayloadInputData::Jumping)
        || input_data.contains(&EnumsPlayerAuthInputPacketPayloadInputData::Startjumping);
    let jumped = input.jumping && !was_jumping;
    input.sneaking = input_data.contains(&EnumsPlayerAuthInputPacketPayloadInputData::Sneaking);
    input.sprinting = input_data.contains(&EnumsPlayerAuthInputPacketPayloadInputData::Sprinting);
    input.tick = pk.client_tick.inputtick as i64;
    // Vertical collision: the move ended against the ground (or a ceiling).
    input.on_ground =
        input_data.contains(&EnumsPlayerAuthInputPacketPayloadInputData::Verticalcollision);

    // Update persistent state flags
    let mut toggle_sneak = None;
    let mut toggle_sprint = None;

    if input_data.contains(&EnumsPlayerAuthInputPacketPayloadInputData::Startsneaking) {
        state.sneaking = true;
        toggle_sneak = Some(true);
    }
    if input_data.contains(&EnumsPlayerAuthInputPacketPayloadInputData::Stopsneaking) {
        state.sneaking = false;
        toggle_sneak = Some(false);
    }
    if input_data.contains(&EnumsPlayerAuthInputPacketPayloadInputData::Startsprinting) {
        state.sprinting = true;
        toggle_sprint = Some(true);
    }
    if input_data.contains(&EnumsPlayerAuthInputPacketPayloadInputData::Stopsprinting) {
        state.sprinting = false;
        toggle_sprint = Some(false);
    }
    if input_data.contains(&EnumsPlayerAuthInputPacketPayloadInputData::Startswimming) {
        state.swimming = true;
    }
    if input_data.contains(&EnumsPlayerAuthInputPacketPayloadInputData::Stopswimming) {
        state.swimming = false;
    }
    if input_data.contains(&EnumsPlayerAuthInputPacketPayloadInputData::Startgliding) {
        state.gliding = true;
    }
    if input_data.contains(&EnumsPlayerAuthInputPacketPayloadInputData::Stopgliding) {
        state.gliding = false;
    }
    if input_data.contains(&EnumsPlayerAuthInputPacketPayloadInputData::Startflying) {
        state.flying = true;
    }
    if input_data.contains(&EnumsPlayerAuthInputPacketPayloadInputData::Stopflying) {
        state.flying = false;
    }

    if let Some(mut fall) = fall {
        fall.after_move(new_pos.y - old_pos.y, input.on_ground, state.flying);
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
    Some(Moved {
        delta: new_pos - old_pos,
        sprinting: state.sprinting,
        swimming: state.swimming,
        flying: state.flying,
        jumped,
    })
}

/// What one accepted `PlayerAuthInput` moved, for food exhaustion.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Moved {
    delta: DVec3,
    sprinting: bool,
    swimming: bool,
    flying: bool,
    jumped: bool,
}

impl Moved {
    /// Exhaustion per `minecraft:exhaustion_values`: swimming per metre
    /// moved, sprinting and walking per horizontal metre, and per jump
    /// (`sprint_jump` while sprinting). Flying costs nothing.
    fn exhaustion(&self, values: &super::hunger::ExhaustionValues) -> f32 {
        if self.flying {
            return 0.0;
        }
        let horizontal = self.delta.x.hypot(self.delta.z) as f32;
        let mut out = if self.swimming {
            self.delta.length() as f32 * values.swim
        } else if self.sprinting {
            horizontal * values.sprint
        } else {
            horizontal * values.walk
        };
        if self.jumped && !self.swimming {
            out += if self.sprinting {
                values.sprint_jump
            } else {
                values.jump
            };
        }
        out
    }
}

/// Process a PlayerAction packet: sprint/sneak state, StartBreak event.
fn process_player_action(
    entity: Entity,
    pk: &jolyne::valentine::PlayerActionPacket,
    players: &mut MovingPlayers,
    events: &mut ResMut<MovementEvents>,
) {
    trace!(action = ?pk.action, "PlayerAction received");

    match pk.action {
        EnumsPlayerActionType::Startjump => {
            trace!("Player jumped");
        }
        EnumsPlayerActionType::Startdestroyblock => {
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
        EnumsPlayerActionType::Startsprinting => {
            if let Ok((_, _, mut state, _, _)) = players.get_mut(entity) {
                state.sprinting = true;
            }
        }
        EnumsPlayerActionType::Stopsprinting => {
            if let Ok((_, _, mut state, _, _)) = players.get_mut(entity) {
                state.sprinting = false;
            }
        }
        EnumsPlayerActionType::Startsneaking => {
            if let Ok((_, _, mut state, _, _)) = players.get_mut(entity) {
                state.sneaking = true;
            }
        }
        EnumsPlayerActionType::Stopsneaking => {
            if let Ok((_, _, mut state, _, _)) = players.get_mut(entity) {
                state.sneaking = false;
            }
        }
        EnumsPlayerActionType::Changedimensionack => {
            trace!("Player acknowledged dimension change");
        }
        EnumsPlayerActionType::Handledteleport => {
            trace!("Player handled teleport");
        }
        _ => {
            trace!(action = ?pk.action, "Unhandled player action");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn moved(delta: DVec3, sprinting: bool, swimming: bool, jumped: bool) -> Moved {
        Moved {
            delta,
            sprinting,
            swimming,
            flying: false,
            jumped,
        }
    }

    #[test]
    fn movement_exhaustion_uses_the_player_definition_values() {
        let values = super::super::hunger::ExhaustionValues::vanilla();
        let step = DVec3::new(3.0, 1.0, 4.0);
        // Walking costs nothing; sprinting 0.1 per horizontal metre.
        assert_eq!(moved(step, false, false, false).exhaustion(&values), 0.0);
        assert!((moved(step, true, false, false).exhaustion(&values) - 0.5).abs() < 1e-6);
        // Swimming 0.01 per metre in 3D.
        let swim = moved(step, false, true, false).exhaustion(&values);
        assert!((swim - 0.01 * 26f32.sqrt()).abs() < 1e-6);
        assert_eq!(
            moved(DVec3::ZERO, false, false, true).exhaustion(&values),
            0.05
        );
        assert_eq!(
            moved(DVec3::ZERO, true, false, true).exhaustion(&values),
            0.2
        );
        let mut flying = moved(step, true, false, true);
        flying.flying = true;
        assert_eq!(flying.exhaustion(&values), 0.0);
    }
}
