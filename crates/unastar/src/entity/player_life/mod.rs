//! Player damage, death and respawn.
//!
//! - [`hurt_player`] applies one [`Hit`] to a player under the native hurt
//!   rules: game-mode immunity, the damage game rules, difficulty scaling of
//!   mob damage, the hurt timer and armour. A full hit shows the hurt event
//!   to the player and its viewers and pushes the player's client.
//! - [`environment`] deals the damage of the world around a player: falls
//!   (tracked from `PlayerAuthInput`), lava, fire, burning and the void.
//! - [`death`] runs when health reaches zero: the death event, the death
//!   message, the drops and the respawn screen.
//! - [`respawn`] answers the client's respawn and puts the player back at
//!   the world spawn.
//! - [`attributes`] replicates the player's attributes to its client.
//!
//! A dead player carries [`Dead`] (the marker dead mobs carry) until it
//! respawns, so every system that skips dead actors skips it too.
//!
//! [`Hit`]: crate::entity::mob::combat::Hit

pub mod attributes;
pub mod constants;
pub mod death;
pub mod environment;
pub mod hurt;
pub mod respawn;
pub mod rules;
#[cfg(test)]
mod tests;

use bevy_ecs::prelude::*;
use jolyne::valentine::McpePacket;

pub use crate::entity::mob::state::Dead;
pub use death::kill_player;
pub use hurt::hurt_player;
pub use rules::{PlayerRules, PlayerRulesError};

use crate::entity::components::{PlayerSession, Position};
use crate::server::broadcast::EntityGrid;

/// Insert the resources the player life systems read: the compiled player
/// definition, the respawn queue and the world spawn (`spawn` uses the
/// players' `Position` convention, eye height above the feet).
pub fn install(world: &mut World, spawn: glam::DVec3) -> Result<(), PlayerRulesError> {
    world.insert_resource(PlayerRules::vanilla()?);
    world.init_resource::<respawn::RespawnQueue>();
    world.insert_resource(respawn::WorldSpawn(spawn));
    Ok(())
}

/// Send `packet` to `player` and to the players that can see it (the
/// players within one chunk of its chunk, as arm swings are relayed).
pub(crate) fn send_to_player_and_viewers(world: &World, player: Entity, packet: &McpePacket) {
    if let Some(session) = world.get::<PlayerSession>(player) {
        let _ = session.send(packet.clone());
    }
    let (Some(position), Some(grid)) = (
        world.get::<Position>(player),
        world.get_resource::<EntityGrid>(),
    ) else {
        return;
    };
    let chunk = (
        (position.0.x.floor() as i32) >> 4,
        (position.0.z.floor() as i32) >> 4,
    );
    for viewer in grid.get_neighbors(chunk, 1) {
        if viewer == player {
            continue;
        }
        if crate::server::broadcast::player_views::can_receive_actor(world, player, viewer)
            && let Some(session) = world.get::<PlayerSession>(viewer)
        {
            let _ = session.send(packet.clone());
        }
    }
}
