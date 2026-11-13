//! The respawn screen and respawning.
//!
//! When a player dies the server opens the respawn screen with `Respawn`
//! searching-for-spawn and then ready-to-spawn, both at its respawn position
//! (its [`PlayerSpawnPoint`], else the world spawn; the layout BDS sends at
//! join: the player's eye position and runtime ID 0).
//! The client answers with `Respawn` client-ready-to-spawn, which the server
//! acknowledges with ready-to-spawn, and `PlayerAction` respawn; the first
//! of them respawns the player ([`respawn`]): full health, fresh hunger,
//! full air, no fire or fall, back at the respawn position (`MovePlayer` in
//! respawn mode), `ActorEvent` spawn-alive and its living box again
//! (`actor_data::respawned`).

use bevy_ecs::prelude::*;
use glam::DVec3;
use jolyne::valentine::types::{
    ActorRuntimeId, EnumsActorEvent, EnumsPlayerPositionModeComponentPositionMode,
    EnumsPlayerRespawnState, Vec2, Vec3,
};
use jolyne::valentine::{McpePacket, MovePlayerPacket, RespawnPacket};

use super::environment::{Burning, PlayerFall};
use super::{Dead, attributes};
use crate::entity::components::{
    Health, Hunger, PlayerSession, Position, Rotation, RuntimeEntityId,
};
use crate::entity::mob::combat::HurtTimer;
use crate::entity::mob::state::LastHurt;

/// Where players respawn, in the players' `Position` convention (eye
/// height above the feet).
#[derive(Resource, Debug, Clone, Copy, PartialEq)]
pub struct WorldSpawn(pub DVec3);

/// A player's own respawn point (`/spawnpoint`), in the same `Position`
/// convention as [`WorldSpawn`]; players without one respawn at the world
/// spawn.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct PlayerSpawnPoint(pub DVec3);

/// Where `player` respawns: its own spawn point, else the world spawn.
pub fn respawn_position(world: &World, player: Entity) -> DVec3 {
    world
        .get::<PlayerSpawnPoint>(player)
        .map_or_else(|| world.resource::<WorldSpawn>().0, |point| point.0)
}

/// A player's request to respawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RespawnRequest {
    /// `Respawn` with the client-ready-to-spawn state.
    ClientReady,
    /// `PlayerAction` respawn.
    Action,
}

/// Respawn requests routed from the network this tick.
#[derive(Resource, Debug, Default)]
pub struct RespawnQueue(pub Vec<(Entity, RespawnRequest)>);

/// A player position (eyes 1.62 above the feet) as BDS reports it: 1.62001
/// above the feet ([`crate::entity::item_drop::PLAYER_REPORTED_EYE_HEIGHT`]).
fn wire(position: DVec3) -> Vec3 {
    use crate::entity::item_drop::{PLAYER_EYE_HEIGHT, PLAYER_REPORTED_EYE_HEIGHT};
    Vec3 {
        x: position.x as f32,
        y: (position.y - PLAYER_EYE_HEIGHT + PLAYER_REPORTED_EYE_HEIGHT) as f32,
        z: position.z as f32,
    }
}

pub(super) fn respawn_packet(spawn: DVec3, state: EnumsPlayerRespawnState) -> McpePacket {
    McpePacket::from(RespawnPacket {
        position: wire(spawn),
        state,
        player_runtime_id: ActorRuntimeId {
            actor_runtime_id: 0,
        },
    })
}

/// Open the respawn screen of a dead `player`.
pub fn open_respawn_screen(world: &World, player: Entity) {
    let spawn = respawn_position(world, player);
    if let Some(session) = world.get::<PlayerSession>(player) {
        for state in [
            EnumsPlayerRespawnState::Searchingforspawn,
            EnumsPlayerRespawnState::Readytospawn,
        ] {
            let _ = session.send(respawn_packet(spawn, state));
        }
    }
}

/// System (exclusive): handle this tick's respawn requests of dead
/// players; requests from living players are ignored.
pub fn apply_respawn_requests(
    world: &mut World,
    mut requests: Local<Vec<(Entity, RespawnRequest)>>,
) {
    std::mem::swap(&mut *requests, &mut world.resource_mut::<RespawnQueue>().0);
    for (player, request) in requests.drain(..) {
        if world.get_entity(player).is_err() || world.get::<Dead>(player).is_none() {
            continue;
        }
        if request == RespawnRequest::ClientReady {
            let spawn = respawn_position(world, player);
            if let Some(session) = world.get::<PlayerSession>(player) {
                let _ = session.send(respawn_packet(spawn, EnumsPlayerRespawnState::Readytospawn));
            }
        }
        respawn(world, player);
    }
}

/// Bring a dead `player` back at its respawn position.
pub fn respawn(world: &mut World, player: Entity) {
    if world.get::<Dead>(player).is_none() {
        return;
    }
    let spawn = respawn_position(world, player);
    {
        let mut entity = world.entity_mut(player);
        entity.remove::<(Dead, Burning, HurtTimer, LastHurt)>();
        if let Some(mut health) = entity.get_mut::<Health>() {
            health.current = health.max;
        }
        if let Some(mut hunger) = entity.get_mut::<Hunger>() {
            *hunger = Hunger::default();
        }
        if let Some(mut fall) = entity.get_mut::<PlayerFall>() {
            *fall = PlayerFall::default();
        }
        if let Some(mut position) = entity.get_mut::<Position>() {
            position.0 = spawn;
        }
    }
    super::breath::refill(world, player);
    let (Some(session), Some(runtime_id)) = (
        world.get::<PlayerSession>(player),
        world.get::<RuntimeEntityId>(player).map(|id| id.0 as u64),
    ) else {
        return;
    };
    let rotation = world.get::<Rotation>(player).copied().unwrap_or_default();
    let _ = session.send(McpePacket::from(MovePlayerPacket {
        player_runtime_id: ActorRuntimeId {
            actor_runtime_id: runtime_id,
        },
        position: wire(spawn),
        rotation: Vec2 {
            x: rotation.pitch,
            y: rotation.yaw,
        },
        y_head_rotation: rotation.head_yaw,
        position_mode: EnumsPlayerPositionModeComponentPositionMode::Respawn,
        on_ground: false,
        riding_runtime_id: ActorRuntimeId {
            actor_runtime_id: 0,
        },
        teleport_data: None,
        tick: crate::entity::components::input_tick_of(world, player),
    }));
    // Viewers see the player come back too (BDS 1.26.51 player-data
    // capture, `kill_on_land_viewer`).
    super::hurt::send_player_event(world, player, EnumsActorEvent::SpawnAlive);
    let hunger = world.get::<Hunger>(player).cloned().unwrap_or_default();
    attributes::send_attributes(
        world,
        player,
        attributes::hunger_attributes(&hunger).to_vec(),
    );
    if let Some(mut sent) = world.get_mut::<crate::server::game::attributes::SentAttributes>(player)
    {
        sent.respawned(&hunger);
    }
    super::actor_data::respawned(world, player);
}
