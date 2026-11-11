//! Other players as each client sees them (BDS 1.26.51).
//!
//! A player is shown to the players in its send range
//! (`entity::send_range`, the chunk circle every actor uses): `AddPlayer`
//! in the tick one comes into range (its feet, its actor data as its
//! viewers last received it, its game type:
//! `broadcast::build_add_player_packet`), `RemoveActor` a tick after it
//! leaves, and when it leaves the game its `ActorViewers` removal sends
//! `RemoveActor` to the rest. Its actor data and attributes reach the same
//! viewers (`player_life::player_viewers`).
//!
//! Every `PlayerAuthInput` a player sends is relayed to its viewers as one
//! `MovePlayer` (mode normal) with the eye position, ground contact and head
//! yaw after the input, the input's own tick, and the pitch and yaw the
//! player had *before* it (the previous input's, or a teleport's), the yaw
//! wrapped into [-180, 180) while the head yaw is passed on as sent. BDS
//! relays every input, also a player's that stands still, at any distance in
//! range, and never sends a player `MoveActorDelta` (two-client captures at
//! 2–56 blocks: one `MovePlayer` per input tick, two in one batch when two
//! inputs arrived in one tick; a player turning 9° per input is relayed with
//! head yaw 9, 18, 27 and yaw 0, 9, 18; head 189 comes with yaw 180 shown
//! as -180). The ground contact is the server's: the feet on a block,
//! whatever the client's collision flags say (BDS reports a still player of
//! the sweep client, which sends none, on the ground, and one 0.04 above
//! the floor while falling in the air).

use std::collections::HashMap;

use bevy_ecs::prelude::*;
use glam::DVec3;
use jolyne::valentine::types::{
    ActorRuntimeId, ActorUniqueId, EnumsPlayerPositionModeComponentPositionMode, PlayerInputTick,
    Vec2, Vec3,
};
use jolyne::valentine::{McpePacket, MovePlayerPacket, RemoveActorPacket};

use crate::entity::PLAYER_EYE_HEIGHT;
use crate::entity::components::{
    ActorViewers, GameMode, Player, PlayerName, PlayerSession, PlayerUuid, Position, Rotation,
    RuntimeEntityId,
};
use crate::entity::player_life::actor_data::{ExplicitGameMode, PlayerActorData};
use crate::entity::send_range::{Candidates, ViewerChanges};
use crate::entity::systems::physics::{BlockSource, collect_block_boxes};
use crate::registry::collision::Aabb;

/// One accepted `PlayerAuthInput`, to relay to the player's viewers.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RelayedMove {
    pub player: Entity,
    /// Eye position after the input.
    pub position: DVec3,
    /// Pitch and yaw before the input.
    pub pitch: f32,
    pub yaw: f32,
    /// Head yaw the input sent.
    pub head_yaw: f32,
    pub tick: u64,
}

/// The inputs accepted this tick, in order (filled by the movement system).
#[derive(Resource, Debug, Default)]
pub struct RelayedMoves(pub Vec<RelayedMove>);

/// How far below the feet a block still counts as ground (float slack:
/// BDS reported a player whose feet were 0.039 above the floor as airborne).
const GROUND_PROBE: f64 = 1.0e-3;

/// Whether a player whose eye is at `eye` stands on a block.
pub fn standing(blocks: &impl BlockSource, eye: DVec3, boxes: &mut Vec<Aabb>) -> bool {
    let feet = eye - DVec3::Y * PLAYER_EYE_HEIGHT;
    let half = 0.3;
    let probe = Aabb::new(
        DVec3::new(feet.x - half, feet.y - GROUND_PROBE, feet.z - half),
        DVec3::new(feet.x + half, feet.y, feet.z + half),
    );
    boxes.clear();
    collect_block_boxes(blocks, probe, boxes);
    boxes.iter().any(|block| block.intersects(&probe))
}

/// Degrees wrapped into [-180, 180).
fn wrap_degrees(degrees: f32) -> f32 {
    (degrees + 180.0).rem_euclid(360.0) - 180.0
}

fn move_player_packet(runtime_id: i64, relayed: &RelayedMove, on_ground: bool) -> MovePlayerPacket {
    MovePlayerPacket {
        player_runtime_id: ActorRuntimeId {
            actor_runtime_id: runtime_id as u64,
        },
        position: Vec3 {
            x: relayed.position.x as f32,
            y: relayed.position.y as f32,
            z: relayed.position.z as f32,
        },
        rotation: Vec2 {
            x: relayed.pitch,
            y: wrap_degrees(relayed.yaw),
        },
        y_head_rotation: relayed.head_yaw,
        position_mode: EnumsPlayerPositionModeComponentPositionMode::Normal,
        on_ground,
        riding_runtime_id: ActorRuntimeId {
            actor_runtime_id: 0,
        },
        teleport_data: None,
        tick: PlayerInputTick {
            inputtick: relayed.tick,
        },
    }
}

/// System: show every player to the players in its send range, and relay
/// each input it sent this tick to them as `MovePlayer`.
#[allow(clippy::type_complexity)]
pub fn sync_player_viewers(
    mut commands: Commands,
    mut players: Query<
        (
            Entity,
            &RuntimeEntityId,
            &PlayerUuid,
            &PlayerName,
            &Position,
            &Rotation,
            &GameMode,
            Has<ExplicitGameMode>,
            &PlayerActorData,
            Option<&mut ActorViewers>,
        ),
        With<Player>,
    >,
    sessions: Query<(Entity, &PlayerSession, Option<&Position>), With<Player>>,
    relayed: Option<ResMut<RelayedMoves>>,
    blocks: crate::entity::systems::physics::WorldBlocks,
    mut boxes: Local<Vec<Aabb>>,
) {
    let candidates = Candidates::new(
        sessions
            .iter()
            .filter_map(|(entity, _, position)| Some((entity, position?.0))),
    );
    let send = |viewer: Entity, packet: &McpePacket| {
        if let Ok((_, session, _)) = sessions.get(viewer) {
            let _ = session.send(packet.clone());
        }
    };
    let mut moves: HashMap<Entity, Vec<RelayedMove>> = HashMap::new();
    if let Some(mut relayed) = relayed {
        for relayed in relayed.0.drain(..) {
            moves.entry(relayed.player).or_default().push(relayed);
        }
    }
    for (
        player,
        runtime_id,
        uuid,
        name,
        position,
        rotation,
        game_mode,
        explicit_game_mode,
        actor_data,
        viewers,
    ) in &mut players
    {
        let Some(mut viewers) = viewers else {
            commands.entity(player).insert(ActorViewers::default());
            continue;
        };
        // Inputs reach the viewers that already see the player.
        for relayed in moves.get(&player).into_iter().flatten() {
            let on_ground = standing(&blocks, relayed.position, &mut boxes);
            let packet = McpePacket::from(move_player_packet(runtime_id.0, relayed, on_ground));
            for viewer in &viewers.viewers {
                send(*viewer, &packet);
            }
        }
        let desired = candidates.in_range_of(position.0, Some(player));
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
        if !changes.fresh.is_empty() {
            let add = McpePacket::from(super::broadcast::build_add_player_packet(
                runtime_id.0,
                uuid.0,
                &name.0,
                position,
                rotation,
                super::broadcast::add_player_game_type(*game_mode, explicit_game_mode),
                actor_data,
            ));
            for viewer in changes.fresh {
                if let Ok((_, session, _)) = sessions.get(viewer)
                    && session.send(add.clone())
                {
                    viewers.viewers.push(viewer);
                }
            }
        }
    }
}
