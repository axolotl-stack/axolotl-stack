//! Movement domain system.
//!
//! Processes PlayerAuthInput and PlayerAction packets, updating position,
//! rotation, input state, the sneak and flight flags and the fall the player
//! life systems resolve. The position is the server's own copy of the
//! player: each input is simulated and the position the client reports is
//! judged against it, as BDS 1.26.51 does (`player_life::authority`, which
//! answers with `CorrectPlayerMovePrediction`); inputs the simulation does
//! not cover are trusted within [`MAX_AUTH_INPUT_MOVE_DELTA_PER_TICK`].
//! A client's flight toggles count only in a game mode that can fly.
//! Each input's move (the copy's) and the sprint, swim and glide toggles it
//! asks for go to [`PlayerMoves`], which `player_life::motion::settle_moves`
//! applies right after (acceptance, `MOVING`, food exhaustion). Forwards
//! block actions from AuthInput to the block packet queue. (`PlayerAction`
//! respawn is routed to `player_life::respawn` instead.)

use bevy_ecs::prelude::*;
use bevy_ecs::query::QueryData;
use glam::DVec3;
use tracing::trace;

/// Farthest one trusted `PlayerAuthInput` may move a player from its
/// server-tracked position (from the server's copy, for a player that has
/// one: [`MAX_TRUSTED_MOVE`]).
const MAX_AUTH_INPUT_MOVE_DELTA_PER_TICK: f64 = MAX_TRUSTED_MOVE as f64;

use super::packet_queues::{
    BlockAction, BlockPacketQueue, MovementEvents, MovementInput, MovementPacketQueue,
};
use crate::ecs::events::ServerEvent;
use crate::entity::components::transform::{Position, Rotation};
use crate::entity::components::{
    ArmourInventory, Effects, GameMode, HeldSlot, MainInventory, PlayerInput, PlayerSession,
    PlayerState,
};
use crate::entity::player_life::authority::{
    LoadedBlocks, MAX_TRUSTED_MOVE, MotionTable, Outcome, PlayerMotion, Situation, WorldTerrain,
    movement_effects_of, movement_gear_of,
};
use crate::entity::player_life::environment::PlayerFall;
use crate::entity::player_life::motion::{Move, PlayerMoves, Toggles, has_elytra};
use crate::entity::player_life::pvp::{InputStart, Swing};
use crate::server::game::types::BlockRegistryResource;
use crate::world::ChunkManager;
use crate::world::ecs::ChunkData;
use jolyne::valentine::types::{EnumsPlayerActionType, EnumsPlayerAuthInputPacketPayloadInputData};
use jolyne::valentine::{CorrectPlayerMovePredictionPacket, McpePacket};

/// What the simulation reads about a player besides its movement.
#[derive(QueryData)]
pub(crate) struct Condition {
    mode: &'static GameMode,
    effects: Option<&'static Effects>,
    /// An elytra, movement enchantments.
    armour: Option<&'static ArmourInventory>,
    /// The held item (a riptide trident).
    hand: Option<(&'static MainInventory, &'static HeldSlot)>,
}

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
        Option<&'static mut PlayerMotion>,
        Option<Condition>,
    ),
>;

/// The world the movement simulation reads, when the server has one.
pub(crate) struct MovementWorld<'a, 'w, 's> {
    chunks: Option<&'a ChunkManager>,
    chunk_data: &'a Query<'w, 's, &'static ChunkData>,
    registry: Option<&'a BlockRegistryResource>,
    table: Option<&'a MotionTable>,
}

impl<'a, 'w, 's> MovementWorld<'a, 'w, 's> {
    /// The blocks and the motion table, when the server has a world.
    fn terrain_parts(&self) -> Option<(LoadedBlocks<'a, 'w, 's>, &'a MotionTable)> {
        Some((
            LoadedBlocks {
                manager: self.chunks?,
                chunks: self.chunk_data,
                registry: &self.registry?.0,
            },
            self.table?,
        ))
    }
}

/// ECS system: drain movement packet queue and apply state changes.
///
/// Runs in `PacketApplySet`. Writes the movement components (Position, Rotation,
/// PlayerState, PlayerInput, PlayerFall, PlayerMoves, PlayerMotion), reads the loaded
/// chunks the movement simulation collides with, and sends each correction through
/// the player's session.
#[allow(clippy::too_many_arguments)]
pub fn apply_movement(
    mut queue: ResMut<MovementPacketQueue>,
    mut block_queue: ResMut<BlockPacketQueue>,
    mut events: ResMut<MovementEvents>,
    mut item_use: Option<ResMut<super::item_use::ItemUseQueue>>,
    mut players: MovingPlayers,
    mut relayed: Option<ResMut<crate::server::player_view::RelayedMoves>>,
    chunks: Option<Res<ChunkManager>>,
    chunk_data: Query<&'static ChunkData>,
    registry: Option<Res<BlockRegistryResource>>,
    table: Option<Res<MotionTable>>,
    sessions: Query<&PlayerSession>,
    mut starts: Query<&mut InputStart>,
    tick: Option<Res<crate::ecs::resources::TickCounter>>,
) {
    let now = tick.map_or(0, |tick| tick.current);
    let _span = tracing::info_span!("apply_movement", count = queue.0.len()).entered();
    let world = MovementWorld {
        chunks: chunks.as_deref(),
        chunk_data: &chunk_data,
        registry: registry.as_deref(),
        table: table.as_deref(),
    };
    for (entity, input) in queue.0.drain(..) {
        match input {
            MovementInput::AuthInput(pk) => {
                // The rotation before this input: what the relayed move
                // carries as the player's pitch and yaw.
                let before = players
                    .get(entity)
                    .ok()
                    .map(|(_, rotation, ..)| (rotation.pitch, rotation.yaw));
                // Combat: the attacker's state an attack of this tick sees
                // (`player_life::pvp`).
                if let (Ok(mut start), Ok((position, _, state, input, fall, _))) =
                    (starts.get_mut(entity), players.get(entity))
                    && start.tick != now
                {
                    *start = InputStart {
                        tick: now,
                        swing: Swing {
                            eye: position.0,
                            on_ground: input.on_ground,
                            falling: fall.is_some_and(|fall| fall.fall.distance > 0.0),
                            sprinting: state.sprinting,
                        },
                    };
                }
                // A rejected input is rejected whole, block actions included.
                let Some(correction) =
                    process_auth_input(entity, &pk, &mut players, &mut events, &world)
                else {
                    continue;
                };
                if let Some(correction) = correction
                    && let Ok(session) = sessions.get(entity)
                {
                    tracing::debug!(
                        entity = ?entity,
                        tick = correction.tick.inputtick,
                        position = ?correction.pos,
                        "Sent CorrectPlayerMovePrediction"
                    );
                    let _ = session.send(McpePacket::from(correction));
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

                // A missed swing plays its sound; a jab packed into the
                // input is the same as one sent on its own.
                if let Some(queue) = item_use.as_mut() {
                    queue
                        .0
                        .extend(item_requests(&pk).map(|request| (entity, request)));
                }

                // Every accepted input goes to the block system, with or
                // without block actions: a break advances once per input.
                block_queue.0.push((
                    entity,
                    BlockAction::AuthInputActions(pk.player_block_actions.unwrap_or_default()),
                ));
            }
            MovementInput::PlayerAction(pk) => {
                process_player_action(entity, &pk, &mut players, &mut events);
            }
        }
    }
}

/// The item-use requests an accepted input carries: the `MissedSwing` flag
/// and an item use `UseAsAttack` packed into the input
/// (`PerformItemInteraction`), which BDS 1.26.51 answers like the
/// standalone transaction (spear capture).
fn item_requests(
    pk: &jolyne::valentine::PlayerAuthInputPacket,
) -> impl Iterator<Item = super::item_use::UseRequest> {
    use super::item_use::UseRequest;
    use jolyne::valentine::types::EnumsItemUseInventoryTransactionActionType as ItemUse;
    let swing = pk
        .input_data
        .contains(&EnumsPlayerAuthInputPacketPayloadInputData::Missedswing)
        .then_some(UseRequest::MissedSwing);
    let jab = pk
        .input_data
        .contains(&EnumsPlayerAuthInputPacketPayloadInputData::Performiteminteraction)
        .then_some(pk.item_use_transaction.as_ref())
        .flatten()
        .filter(|packed| packed.item_use_transaction.action_type == ItemUse::Useasattack)
        .map(|_| UseRequest::AttackAir);
    jab.into_iter().chain(swing)
}

/// Process a PlayerAuthInput packet: position, rotation, input flags, the
/// sneak and flight toggles; the move and the other toggles go to
/// [`PlayerMoves`]. Returns the correction to send, if any, or `None` for
/// an input rejected whole.
fn process_auth_input(
    entity: Entity,
    pk: &jolyne::valentine::PlayerAuthInputPacket,
    players: &mut MovingPlayers,
    events: &mut ResMut<MovementEvents>,
    world: &MovementWorld<'_, '_, '_>,
) -> Option<Option<CorrectPlayerMovePredictionPacket>> {
    let Ok((mut pos, mut rot, mut state, mut input, fall, moves, motion, condition)) =
        players.get_mut(entity)
    else {
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

    // The flight this input asks for, in a game mode that can fly (applied
    // once the input is accepted): a flying player's input is trusted.
    let input_data = &pk.input_data;
    let has = |flag| input_data.contains(&flag);
    use EnumsPlayerAuthInputPacketPayloadInputData as Flag;
    let may_fly = condition
        .as_ref()
        .is_some_and(|condition| condition.mode.allows_flight());
    let flying = may_fly && (state.flying || has(Flag::Startflying)) && !has(Flag::Stopflying);

    let old_pos = pos.0;
    let situation = condition
        .as_ref()
        .map_or_else(Situation::default, |condition| {
            situation_of(condition, flying)
        });
    let outcome = judge_position(entity, pk, old_pos, motion, &situation, world)?;
    let new_pos = outcome.position;
    pos.0 = new_pos;
    state.flying = flying;

    // Update rotation
    rot.pitch = pk.player_rotation.x;
    rot.yaw = pk.player_rotation.y;
    rot.head_yaw = pk.player_head_rotation;

    // Update input
    let was_jumping = input.jumping;
    input.move_x = pk.move_vector.x;
    input.move_z = pk.move_vector.y;
    input.jumping = input_data.contains(&EnumsPlayerAuthInputPacketPayloadInputData::Jumping)
        || input_data.contains(&EnumsPlayerAuthInputPacketPayloadInputData::Startjumping);
    let jumped = input.jumping && !was_jumping;
    input.sneaking = input_data.contains(&EnumsPlayerAuthInputPacketPayloadInputData::Sneaking);
    input.sprinting = input_data.contains(&EnumsPlayerAuthInputPacketPayloadInputData::Sprinting);
    input.tick = pk.client_tick.inputtick as i64;
    // Kept by a knockback (`player_life::hurt`); bounded like a move.
    input.velocity = DVec3::new(
        f64::from(pk.pos_delta.x),
        f64::from(pk.pos_delta.y),
        f64::from(pk.pos_delta.z),
    )
    .clamp(
        DVec3::splat(-MAX_AUTH_INPUT_MOVE_DELTA_PER_TICK),
        DVec3::splat(MAX_AUTH_INPUT_MOVE_DELTA_PER_TICK),
    );
    // The ground contact after the move: the copy's (a trusted input's
    // vertical collision).
    input.on_ground = outcome.on_ground;

    // The sneak toggles apply here; sprint, swim and glide are settled with
    // the world at hand (`player_life::motion`).
    let mut toggle_sneak = None;
    if has(Flag::Startsneaking) {
        state.sneaking = true;
        toggle_sneak = Some(true);
    }
    if has(Flag::Stopsneaking) {
        state.sneaking = false;
        toggle_sneak = Some(false);
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
                jumped: outcome.jumped,
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
    Some(outcome.correction)
}

/// The input's outcome: the server's copy judges it, or, for a player
/// without one (no world to simulate in), the reported position is taken
/// within [`MAX_AUTH_INPUT_MOVE_DELTA_PER_TICK`] of `old_pos`. `None`
/// rejects the input whole.
fn judge_position(
    entity: Entity,
    pk: &jolyne::valentine::PlayerAuthInputPacket,
    old_pos: DVec3,
    motion: Option<Mut<PlayerMotion>>,
    situation: &Situation,
    world: &MovementWorld<'_, '_, '_>,
) -> Option<Outcome> {
    let reported = DVec3::new(
        f64::from(pk.position.x),
        f64::from(pk.position.y),
        f64::from(pk.position.z),
    );
    let outcome = match (motion, world.terrain_parts()) {
        (Some(mut motion), Some((blocks, table))) => {
            let terrain = WorldTerrain {
                blocks: &blocks,
                table,
            };
            motion.input(pk, &terrain, situation)
        }
        _ => (old_pos.distance(reported) <= MAX_AUTH_INPUT_MOVE_DELTA_PER_TICK)
            .then(|| Outcome::reported(pk)),
    };
    if outcome.is_none() {
        tracing::warn!(
            entity = ?entity,
            from = ?old_pos,
            to = ?reported,
            "Rejected implausible PlayerAuthInput movement delta"
        );
    }
    outcome
}

/// What the movement authority reads about a player besides its movement;
/// `flying` is the flight the input asks for, in a game mode that can fly.
fn situation_of(condition: &ConditionItem<'_, '_>, flying: bool) -> Situation {
    Situation {
        flight: flying || *condition.mode == GameMode::Spectator,
        glide: condition.armour.is_some_and(has_elytra),
        effects: movement_effects_of(condition.effects),
        gear: movement_gear_of(
            condition.armour,
            condition
                .hand
                .and_then(|(inventory, held)| inventory.hotbar(held.0)),
        ),
    }
}

/// Whether every value the server takes from a `PlayerAuthInput` (position, rotation,
/// head yaw, move vector and the velocity a trusted input reports) is finite.
fn auth_input_is_finite(pk: &jolyne::valentine::PlayerAuthInputPacket) -> bool {
    [
        pk.position.x,
        pk.position.y,
        pk.position.z,
        pk.pos_delta.x,
        pk.pos_delta.y,
        pk.pos_delta.z,
        pk.player_rotation.x,
        pk.player_rotation.y,
        pk.player_head_rotation,
        pk.move_vector.x,
        pk.move_vector.y,
        pk.pos_delta.x,
        pk.pos_delta.y,
        pk.pos_delta.z,
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
        let mut falling = moving.clone();
        falling.move_vector.x = 0.0;
        falling.pos_delta.y = f32::NAN;
        for input in [moving, falling] {
            let (pos, _) = apply(
                &mut world,
                player,
                MovementInput::AuthInput(Box::new(input)),
            );
            assert_eq!(pos, START);
        }
        assert!(world.resource::<MovementEvents>().0.is_empty());
    }
}
