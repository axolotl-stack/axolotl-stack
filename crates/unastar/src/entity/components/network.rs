//! Client-side visibility of actors (players included).

use bevy_ecs::lifecycle::HookContext;
use bevy_ecs::prelude::*;
use bevy_ecs::world::DeferredWorld;
use glam::DVec3;
use jolyne::valentine::types::ActorUniqueId;
use jolyne::valentine::{McpePacket, RemoveActorPacket};

use super::{PlayerSession, RuntimeEntityId, RuntimeId};

/// Players that currently have this actor spawned on their client, plus the
/// last state sent to them.
///
/// The viewer systems (`server::broadcast::sync_actor_viewers`,
/// `mob::net::network::sync_mob_viewers`, `server::player_view`) add and
/// remove viewers by the send range (`entity::send_range`). Removing the
/// component (or despawning the entity) sends `RemoveActor` to every
/// remaining viewer.
#[derive(Component, Debug, Default, Clone)]
#[component(on_remove = actor_viewers_on_remove)]
pub struct ActorViewers {
    pub viewers: Vec<Entity>,
    /// Viewers that went out of range this tick; they get `RemoveActor`
    /// in the next one if they are still out (see `send_range`).
    pub leaving: Vec<Entity>,
    /// Feet position last broadcast.
    pub last_position: DVec3,
    /// Velocity last broadcast.
    pub last_velocity: DVec3,
    /// Stack size last broadcast (item actors only).
    pub last_stack_size: u8,
}

fn actor_viewers_on_remove(world: DeferredWorld<'_>, context: HookContext) {
    let entity = context.entity;
    let Some(viewers) = world.get::<ActorViewers>(entity) else {
        return;
    };
    // Players carry their id as `RuntimeEntityId`.
    let Some(unique_id) = world
        .get::<RuntimeId>(entity)
        .map(|id| id.0)
        .or_else(|| world.get::<RuntimeEntityId>(entity).map(|id| id.0))
    else {
        return;
    };
    let packet = RemoveActorPacket {
        target_actor_id: ActorUniqueId {
            actor_unique_id: unique_id,
        },
    };
    for viewer in &viewers.viewers {
        if let Some(session) = world.get::<PlayerSession>(*viewer) {
            let _ = session.send(McpePacket::from(packet.clone()));
        }
    }
}
