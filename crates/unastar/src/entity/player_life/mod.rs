//! Player damage, death and respawn.
//!
//! - [`hurt_player`] applies one [`Hit`] to a player under the native hurt
//!   rules: game-mode immunity, the damage game rules, difficulty scaling of
//!   mob damage, the hurt timer and armour. A full hit shows the hurt event
//!   to the player and its viewers and pushes the player's client.
//! - [`environment`] deals the damage of the world around a player: falls
//!   (tracked from `PlayerAuthInput`), lava, fire, burning and the void.
//! - [`breath`] keeps the air supply and the `BREATHING` flag: drowning
//!   under water, refilling above it and in bubble columns.
//! - [`authority`] keeps the server's own copy of the player's movement:
//!   every input simulated as BDS simulates it, the client's position
//!   taken, pulled towards or corrected, teleports enforced.
//! - [`motion`] settles what the movement inputs ask for: the sprint, swim
//!   and glide toggles BDS accepts or refuses, `MOVING` and the food
//!   exhaustion of each move.
//! - [`actor_data`] keeps the player's synched actor data (flags, air, box,
//!   pose, last death) and sends its changes, its join `SetActorData` and
//!   the rows `AddPlayer` carries.
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

pub mod actor_data;
pub mod attributes;
pub mod authority;
pub mod breath;
pub mod constants;
pub mod death;
pub mod environment;
pub mod hurt;
pub mod motion;
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

use crate::entity::components::{ActorViewers, Player, PlayerSession, Position};
use crate::entity::send_range;

/// Insert the resources the player life systems read: the compiled player
/// definition, the respawn queue and the world spawn (`spawn` uses the
/// players' `Position` convention, eye height above the feet).
pub fn install(world: &mut World, spawn: glam::DVec3) -> Result<(), PlayerRulesError> {
    world.insert_resource(PlayerRules::vanilla()?);
    world.init_resource::<respawn::RespawnQueue>();
    world.insert_resource(respawn::WorldSpawn(spawn));
    Ok(())
}

/// The players that see `player`: the players it is shown to
/// (`ActorViewers`, once players carry it) or, for a player without it,
/// every player in the actor send range of it ([`crate::entity::send_range`],
/// BDS 1.26.51's rule for every actor). Its actor data and attributes reach
/// exactly these players.
pub fn player_viewers(world: &World, player: Entity) -> Vec<Entity> {
    if let Some(viewers) = world.get::<ActorViewers>(player) {
        return viewers.viewers.clone();
    }
    let Some(position) = world.get::<Position>(player) else {
        return Vec::new();
    };
    let actor = send_range::chunk_of(position.0);
    let Some(mut players) = world.try_query_filtered::<(Entity, &Position), With<Player>>() else {
        return Vec::new();
    };
    players
        .iter(world)
        .filter(|(viewer, at)| {
            *viewer != player && send_range::in_range(send_range::chunk_of(at.0), actor)
        })
        .map(|(viewer, _)| viewer)
        .collect()
}

/// Send `packet` to `player` and to the players that see it.
pub(crate) fn send_to_player_and_viewers(world: &World, player: Entity, packet: &McpePacket) {
    if let Some(session) = world.get::<PlayerSession>(player) {
        let _ = session.send(packet.clone());
    }
    for viewer in player_viewers(world, player) {
        if let Some(session) = world.get::<PlayerSession>(viewer) {
            let _ = session.send(packet.clone());
        }
    }
}
