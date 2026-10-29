//! The respawn screen and respawning.
//!
//! When a player dies the server opens the respawn screen with `Respawn`
//! searching-for-spawn and then ready-to-spawn, both at the world spawn (the
//! layout BDS sends at join: the player's eye position and runtime ID 0).
//! The client answers with `Respawn` client-ready-to-spawn, which the server
//! acknowledges with ready-to-spawn, and `PlayerAction` respawn; the first
//! of them respawns the player ([`respawn`]): full health, fresh hunger, no
//! fire or fall, back at the world spawn (`MovePlayer` in respawn mode) and
//! `ActorEvent` spawn-alive.

use bevy_ecs::prelude::*;
use glam::DVec3;
use jolyne::valentine::types::{
    ActorRuntimeId, EnumsActorEvent, EnumsPlayerPositionModeComponentPositionMode,
    EnumsPlayerRespawnState, PlayerInputTick, Vec2, Vec3,
};
use jolyne::valentine::{ActorEventPacket, McpePacket, MovePlayerPacket, RespawnPacket};

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
    let spawn = world.resource::<WorldSpawn>().0;
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
            let spawn = world.resource::<WorldSpawn>().0;
            if let Some(session) = world.get::<PlayerSession>(player) {
                let _ = session.send(respawn_packet(spawn, EnumsPlayerRespawnState::Readytospawn));
            }
        }
        respawn(world, player);
    }
}

/// Bring a dead `player` back at the world spawn.
pub fn respawn(world: &mut World, player: Entity) {
    if world.get::<Dead>(player).is_none() {
        return;
    }
    let spawn = world.resource::<WorldSpawn>().0;
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
        tick: PlayerInputTick { inputtick: 0 },
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
}
