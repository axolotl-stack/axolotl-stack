//! Level sounds and level events the mob runtime sends: to every player
//! viewing the chunk the event happens in (the players an actor there is
//! replicated to).

use bevy_ecs::prelude::*;
use glam::DVec3;
use jolyne::valentine::types::Vec3;
use jolyne::valentine::{LevelEventPacket, LevelSoundEventPacket, McpePacket};

use crate::entity::components::{Player, PlayerSession, RuntimeEntityId, RuntimeId};
use crate::entity::definition::flags::ActorFlags;
use crate::entity::mob::MobKind;
use crate::world::ChunkManager;
use crate::world::ecs::{ChunkViewers, world_to_chunk_coords};

fn wire(position: DVec3) -> Vec3 {
    Vec3 {
        x: position.x as f32,
        y: position.y as f32,
        z: position.z as f32,
    }
}

/// Send `packet` to every player viewing the chunk containing `position`.
pub fn send_to_viewers(world: &World, position: DVec3, packet: &McpePacket) {
    let (cx, cz) = world_to_chunk_coords(position.x.floor() as i32, position.z.floor() as i32);
    let Some(viewers) = world
        .get_resource::<ChunkManager>()
        .and_then(|chunks| chunks.get_by_coords(cx, cz))
        .and_then(|chunk| world.get::<ChunkViewers>(chunk))
    else {
        return;
    };
    for viewer in viewers.iter() {
        if crate::server::game::player_events::initial_state_pending(world, viewer) {
            continue;
        }
        if let Some(session) = world.get::<PlayerSession>(viewer) {
            let _ = session.send(packet.clone());
        }
    }
}

/// Play the named level sound at `position`, made by `source` (its actor
/// identifier, baby flag and unique ID go into the packet; the client picks
/// the actor's sound variant from them). `data` is the extra data (`-1`
/// for none, a block runtime ID for block sounds).
pub fn play_level_sound(
    world: &World,
    source: Option<Entity>,
    position: DVec3,
    sound: &str,
    data: i32,
) {
    let identifier = source.and_then(|actor| {
        world
            .get::<MobKind>(actor)
            .map(|kind| kind.0.identifier.as_ref())
            .or_else(|| world.get::<Player>(actor).map(|_| "minecraft:player"))
    });
    let unique_id = source.and_then(|actor| {
        world
            .get::<RuntimeId>(actor)
            .map(|id| id.0)
            .or_else(|| world.get::<RuntimeEntityId>(actor).map(|id| id.0))
    });
    let is_baby = source
        .and_then(|actor| world.get::<ActorFlags>(actor))
        .is_some_and(|flags| {
            flags
                .get(jolyne::valentine::actor_flags::BABY)
                .unwrap_or(false)
        });
    let packet = McpePacket::from(LevelSoundEventPacket {
        sound_event: sound.into(),
        position: wire(position),
        data,
        actor_identifier: identifier.unwrap_or_default().into(),
        is_baby,
        is_global: false,
        actor_unique_id: unique_id.unwrap_or(-1),
        fire_at_position: None,
    });
    send_to_viewers(world, position, &packet);
}

/// Broadcast `LevelEvent(event_id, data)` at `position`.
pub fn level_event(world: &World, position: DVec3, event_id: i32, data: i32) {
    let packet = McpePacket::from(LevelEventPacket {
        event_id,
        position: wire(position),
        data,
    });
    send_to_viewers(world, position, &packet);
}

/// The middle of an actor's body (its feet plus half its box height).
pub fn body_centre(world: &World, actor: Entity) -> Option<DVec3> {
    let feet = world.get::<crate::entity::components::Position>(actor)?.0;
    let half = world
        .get::<crate::entity::components::PhysicsBody>(actor)
        .map_or(0.0, |body| body.height / 2.0);
    Some(feet + DVec3::Y * half)
}
