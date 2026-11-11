//! Movement domain system.
//!
//! Processes PlayerAuthInput and PlayerAction packets, updating position,
//! rotation, input state, the sneak and flight flags and the fall the player
//! life systems resolve. Each accepted input's move and the sprint, swim and
//! glide toggles it asks for go to [`PlayerMoves`], which
//! `player_life::motion::settle_moves` applies right after (acceptance,
//! `MOVING`, food exhaustion). Forwards block actions from AuthInput to the
//! block packet queue. (`PlayerAction` respawn is routed to
//! `player_life::respawn` instead.)

use bevy_ecs::prelude::*;
use glam::DVec3;
use tracing::trace;

/// Farthest one `PlayerAuthInput` may move a player from its server-tracked position.
const MAX_AUTH_INPUT_MOVE_DELTA_PER_TICK: f64 = 64.0;

use super::packet_queues::{
    BlockAction, BlockPacketQueue, MovementEvents, MovementInput, MovementPacketQueue,
};
use crate::ecs::events::ServerEvent;
use crate::entity::components::transform::{Position, Rotation};
use crate::entity::components::{PlayerInput, PlayerState};
use crate::entity::player_life::environment::PlayerFall;
use crate::entity::player_life::motion::{Move, PlayerMoves, Toggles};
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
        Option<&'static mut PlayerMoves>,
    ),
>;

/// ECS system: drain movement packet queue and apply state changes.
///
/// Runs in `PacketApplySet`. Can execute in parallel with inventory, chat, and chunks
/// because it writes disjoint components (Position, Rotation, PlayerState, PlayerInput,
/// PlayerFall, PlayerMoves).
pub fn apply_movement(
    mut queue: ResMut<MovementPacketQueue>,
    mut block_queue: ResMut<BlockPacketQueue>,
    mut events: ResMut<MovementEvents>,
    mut players: MovingPlayers,
    mut relayed: Option<ResMut<crate::server::player_view::RelayedMoves>>,
) {
    let _span = tracing::info_span!("apply_movement", count = queue.0.len()).entered();
    for (entity, input) in queue.0.drain(..) {
        match input {
            MovementInput::AuthInput(pk) => {
                // The rotation before this input: what the relayed move
                // carries as the player's pitch and yaw.
                let before = players
                    .get(entity)
                    .ok()
                    .map(|(_, rotation, ..)| (rotation.pitch, rotation.yaw));
                // A rejected input is rejected whole, block actions included.
                if process_auth_input(entity, &pk, &mut players, &mut events).is_none() {
                    continue;
                }
                // Every accepted input is relayed to the player's viewers.
                if let (Some(relayed), Some((pitch, yaw)), Ok((position, rotation, ..))) =
                    (relayed.as_mut(), before, players.get(entity))
                {
                    relayed.0.push(crate::server::player_view::RelayedMove {
                        player: entity,
                        position: position.0,
                        pitch,
                        yaw,
                        head_yaw: rotation.head_yaw,
                        tick: pk.client_tick.inputtick,
                    });
                }

                // Forward block actions to block queue
                if let Some(block_actions) = pk.player_block_actions
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

/// Process a PlayerAuthInput packet: position, rotation, input flags, the
/// sneak and flight toggles; the move and the other toggles go to
/// [`PlayerMoves`].
fn process_auth_input(
    entity: Entity,
    pk: &jolyne::valentine::PlayerAuthInputPacket,
    players: &mut MovingPlayers,
    events: &mut ResMut<MovementEvents>,
) -> Option<()> {
    let Ok((mut pos, mut rot, mut state, mut input, fall, moves)) = players.get_mut(entity) else {
        return None;
    };

    // NaN compares false with everything, so a non-finite value would slip past the delta
    // check below and poison the stored position, rotation or input for good.
    if !auth_input_is_finite(pk) {
        tracing::warn!(
            entity = ?entity,
            position = ?pk.position,
            rotation = ?pk.player_rotation,
            head_yaw = pk.player_head_rotation,
            move_vector = ?pk.move_vector,
            "Rejected non-finite PlayerAuthInput"
        );
        return None;
    }

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

    // The sneak and flight toggles apply here; sprint, swim and glide are
    // settled with the world at hand (`player_life::motion`).
    let has = |flag| input_data.contains(&flag);
    use EnumsPlayerAuthInputPacketPayloadInputData as Flag;
    let mut toggle_sneak = None;
    if has(Flag::Startsneaking) {
        state.sneaking = true;
        toggle_sneak = Some(true);
    }
    if has(Flag::Stopsneaking) {
        state.sneaking = false;
        toggle_sneak = Some(false);
    }
    if has(Flag::Startflying) {
        state.flying = true;
    }
    if has(Flag::Stopflying) {
        state.flying = false;
    }

    if let Some(mut fall) = fall {
        fall.after_move(
            new_pos.y - old_pos.y,
            input.on_ground,
            state.flying,
            state.gliding,
        );
    }
    if let Some(mut moves) = moves {
        moves.record(
            Move {
                delta: new_pos - old_pos,
                jumped: has(Flag::Startjumping),
                on_ground: input.on_ground,
            },
            pk.move_vector.y > 0.0,
            Toggles {
                start_sprint: has(Flag::Startsprinting),
                stop_sprint: has(Flag::Stopsprinting),
                start_swim: has(Flag::Startswimming),
                stop_swim: has(Flag::Stopswimming),
                start_glide: has(Flag::Startgliding),
                stop_glide: has(Flag::Stopgliding),
            },
        );
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
    Some(())
}

/// Whether every value the server takes from a `PlayerAuthInput` (position, rotation,
/// head yaw and move vector) is finite.
fn auth_input_is_finite(pk: &jolyne::valentine::PlayerAuthInputPacket) -> bool {
    [
        pk.position.x,
        pk.position.y,
        pk.position.z,
        pk.player_rotation.x,
        pk.player_rotation.y,
        pk.player_head_rotation,
        pk.move_vector.x,
        pk.move_vector.y,
    ]
    .iter()
    .all(|value| value.is_finite())
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
        EnumsPlayerActionType::Startsneaking => {
            if let Ok((_, _, mut state, ..)) = players.get_mut(entity) {
                state.sneaking = true;
            }
        }
        EnumsPlayerActionType::Stopsneaking => {
            if let Ok((_, _, mut state, ..)) = players.get_mut(entity) {
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
    use bevy_ecs::system::RunSystemOnce;
    use jolyne::valentine::PlayerAuthInputPacket;
    use jolyne::valentine::types::{Vec2, Vec3};

    const START: DVec3 = DVec3::new(0.5, 70.0, 0.5);

    /// A world with one player at [`START`] and the resources `apply_movement` reads.
    fn movement_world() -> (World, Entity) {
        let mut world = World::new();
        world.init_resource::<MovementPacketQueue>();
        world.init_resource::<BlockPacketQueue>();
        world.init_resource::<MovementEvents>();
        let player = world
            .spawn((
                Position(START),
                Rotation::new(10.0, 20.0),
                PlayerState::default(),
                PlayerInput::default(),
            ))
            .id();
        (world, player)
    }

    fn auth_input(position: [f32; 3], rotation: [f32; 2], head_yaw: f32) -> MovementInput {
        MovementInput::AuthInput(Box::new(PlayerAuthInputPacket {
            position: Vec3 {
                x: position[0],
                y: position[1],
                z: position[2],
            },
            player_rotation: Vec2 {
                x: rotation[0],
                y: rotation[1],
            },
            player_head_rotation: head_yaw,
            // Every input carries one block action, forwarded only if the input is accepted.
            player_block_actions: Some(vec![jolyne::valentine::PlayerBlockActionData {
                player_action_type: EnumsPlayerActionType::Startdestroyblock,
                ..Default::default()
            }]),
            ..Default::default()
        }))
    }

    fn forwarded_block_actions(world: &mut World) -> usize {
        world.resource_mut::<BlockPacketQueue>().0.drain(..).count()
    }

    /// Apply one input and return the player's position and (pitch, yaw, head yaw) after.
    fn apply(world: &mut World, player: Entity, input: MovementInput) -> (DVec3, [f32; 3]) {
        world
            .resource_mut::<MovementPacketQueue>()
            .0
            .push((player, input));
        world.run_system_once(apply_movement).unwrap();
        let rotation = *world.get::<Rotation>(player).unwrap();
        (
            world.get::<Position>(player).unwrap().0,
            [rotation.pitch, rotation.yaw, rotation.head_yaw],
        )
    }

    #[test]
    fn moves_within_the_delta_are_accepted_and_larger_ones_rejected() {
        let (mut world, player) = movement_world();
        // 63 blocks along x is inside the 64-block limit.
        let (pos, rotation) = apply(
            &mut world,
            player,
            auth_input([63.5, 70.0, 0.5], [5.0, 90.0], 95.0),
        );
        assert_eq!(pos, DVec3::new(63.5, 70.0, 0.5));
        assert_eq!(rotation, [5.0, 90.0, 95.0]);
        assert_eq!(world.resource::<MovementEvents>().0.len(), 1);
        assert_eq!(forwarded_block_actions(&mut world), 1);

        // 100 blocks from the tracked position is rejected whole: no move, no rotation.
        world.resource_mut::<MovementEvents>().0.clear();
        let (pos, rotation) = apply(
            &mut world,
            player,
            auth_input([163.5, 70.0, 0.5], [0.0, 0.0], 0.0),
        );
        assert_eq!(pos, DVec3::new(63.5, 70.0, 0.5));
        assert_eq!(rotation, [5.0, 90.0, 95.0]);
        assert!(world.resource::<MovementEvents>().0.is_empty());
        assert_eq!(
            forwarded_block_actions(&mut world),
            0,
            "rejected inputs act on no block"
        );
    }

    #[test]
    fn non_finite_positions_and_rotations_are_rejected() {
        let (mut world, player) = movement_world();
        let inputs = [
            auth_input([f32::NAN, 70.0, 0.5], [0.0, 0.0], 0.0),
            auth_input([0.5, f32::INFINITY, 0.5], [0.0, 0.0], 0.0),
            auth_input([0.5, 70.0, f32::NEG_INFINITY], [0.0, 0.0], 0.0),
            auth_input([0.5, 71.0, 0.5], [f32::NAN, 0.0], 0.0),
            auth_input([0.5, 71.0, 0.5], [0.0, f32::INFINITY], 0.0),
            auth_input([0.5, 71.0, 0.5], [0.0, 0.0], f32::NAN),
        ];
        for input in inputs {
            let (pos, rotation) = apply(&mut world, player, input);
            assert_eq!(pos, START);
            assert_eq!(rotation, [20.0, 10.0, 10.0]);
            assert_eq!(
                forwarded_block_actions(&mut world),
                0,
                "rejected inputs act on no block"
            );
        }
        let mut moving = PlayerAuthInputPacket {
            position: Vec3 {
                x: 0.5,
                y: 71.0,
                z: 0.5,
            },
            ..Default::default()
        };
        moving.move_vector.x = f32::NAN;
        let (pos, _) = apply(
            &mut world,
            player,
            MovementInput::AuthInput(Box::new(moving)),
        );
        assert_eq!(pos, START);
        assert!(world.resource::<MovementEvents>().0.is_empty());
    }
}
