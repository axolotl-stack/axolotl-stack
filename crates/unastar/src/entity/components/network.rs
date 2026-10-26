//! Client-side visibility of non-player actors.

use bevy_ecs::lifecycle::HookContext;
use bevy_ecs::prelude::*;
use bevy_ecs::world::DeferredWorld;
use glam::DVec3;
use jolyne::valentine::types::ActorUniqueId;
use jolyne::valentine::{McpePacket, RemoveActorPacket};

use super::{PlayerSession, RuntimeId};

/// Players that currently have this actor spawned on their client, plus the
/// last state sent to them.
///
/// [`crate::server::broadcast::sync_actor_viewers`] adds and removes viewers
/// as players load/unload the actor's chunk. Removing the component (or
/// despawning the entity) sends `RemoveActor` to every remaining viewer.
#[derive(Component, Debug, Default, Clone)]
#[component(on_remove = actor_viewers_on_remove)]
pub struct ActorViewers {
    pub viewers: Vec<Entity>,
    /// Feet position last broadcast.
    pub last_position: DVec3,
    /// Velocity last broadcast.
    pub last_velocity: DVec3,
    /// Stack size last broadcast (item actors only).
    pub last_stack_size: u8,
}

fn actor_viewers_on_remove(world: DeferredWorld<'_>, context: HookContext) {
    let entity = context.entity;
    let (Some(viewers), Some(runtime_id)) = (
        world.get::<ActorViewers>(entity),
        world.get::<RuntimeId>(entity),
    ) else {
        return;
    };
    let packet = RemoveActorPacket {
        target_actor_id: ActorUniqueId {
            actor_unique_id: runtime_id.0,
        },
    };
    for viewer in &viewers.viewers {
        if let Some(session) = world.get::<PlayerSession>(*viewer) {
            let _ = session.send(McpePacket::from(packet.clone()));
        }
    }
}
