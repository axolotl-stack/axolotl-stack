//! The respawn screen and respawning.
//!
//! When a player dies the server opens the respawn screen with `Respawn`
//! searching-for-spawn and then ready-to-spawn, both at its respawn position
//! (its [`PlayerSpawnPoint`], else the world spawn; the layout BDS sends at
//! join: the player's eye position and runtime ID 0).
//! The client answers with `Respawn` client-ready-to-spawn, which the server
//! acknowledges with ready-to-spawn, and `PlayerAction` respawn; the first
//! of them respawns the player ([`respawn`]): full health, fresh hunger, no
//! fire or fall, back at the respawn position (`MovePlayer` in respawn mode)
//! and `ActorEvent` spawn-alive.

use bevy_ecs::prelude::*;
use glam::DVec3;
use jolyne::valentine::types::{
    ActorRuntimeId, EnumsActorEvent, EnumsPlayerPositionModeComponentPositionMode,
    EnumsPlayerRespawnState, Vec2, Vec3,
};
use jolyne::valentine::{ActorEventPacket, McpePacket, MovePlayerPacket, RespawnPacket};

use super::environment::{Burning, PlayerFall};
use super::{Dead, attributes, hooks};
use crate::entity::components::{
    Health, Hunger, Player, PlayerSession, Position, Rotation, RuntimeEntityId,
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

fn wire(position: DVec3) -> Vec3 {
    Vec3 {
        x: position.x as f32,
        y: position.y as f32,
        z: position.z as f32,
    }
}

fn respawn_packet(spawn: DVec3, state: EnumsPlayerRespawnState) -> McpePacket {
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
        try_respawn(world, player, request == RespawnRequest::ClientReady);
    }
}

/// Bring a dead `player` back at its respawn position.
pub fn respawn(world: &mut World, player: Entity) {
    try_respawn(world, player, false);
}

/// Validate a proposed destination before encoding single-precision packets.
fn valid_destination(position: DVec3) -> bool {
    position.is_finite() && position.as_vec3().is_finite()
}

/// Whether the subject is still a dead player with the state needed to respawn.
fn can_respawn(world: &World, player: Entity) -> bool {
    world.get::<Dead>(player).is_some()
        && world.get::<Player>(player).is_some()
        && world.get::<Position>(player).is_some()
        && world
            .get::<Health>(player)
            .is_some_and(|health| health.max.is_finite() && health.max > 0.0)
}

/// Run the decision hook once, then acknowledge and commit an allowed respawn.
fn try_respawn(world: &mut World, player: Entity, acknowledge: bool) {
    if !can_respawn(world, player) {
        return;
    }
    let mut event = hooks::PlayerRespawn::new(player, respawn_position(world, player));
    hooks::respawn(world, &mut event);
    if event.is_cancelled() || !can_respawn(world, player) || !valid_destination(event.position) {
        return;
    }
    let spawn = event.position;
    if acknowledge && let Some(session) = world.get::<PlayerSession>(player) {
        let _ = session.send(respawn_packet(spawn, EnumsPlayerRespawnState::Readytospawn));
    }
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
    crate::entity::freezing::reset_player(world, player);
    send_respawn(world, player, spawn);
    hooks::respawned(
        world,
        hooks::PlayerRespawned {
            player,
            position: spawn,
        },
    );
}

/// Send the normal client updates for a committed respawn.
fn send_respawn(world: &mut World, player: Entity, spawn: DVec3) {
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
    let _ = session.send(McpePacket::from(ActorEventPacket {
        target_runtime_id: ActorRuntimeId {
            actor_runtime_id: runtime_id,
        },
        event_id: EnumsActorEvent::SpawnAlive,
        data: 0,
        fire_at_position: None,
    }));
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
}
