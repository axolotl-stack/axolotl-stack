//! Item cooldowns: a category (shared by every item in it) blocked until a
//! server tick, announced to the player with `PlayerStartItemCooldown`.
//!
//! Categories BDS 1.26.51 was measured to send: `minecraft:ender_pearl`
//! (20 ticks). The goat horn (`minecraft:goat_horn`, 140 ticks) and a
//! shield disabled by an axe (`minecraft:shield`, 100 ticks) use the same
//! naming; their names and durations are public references until a capture
//! confirms them.

use bevy_ecs::prelude::*;

use crate::entity::components::PlayerSession;

/// Item cooldowns of a player: category and the server tick it ends.
#[derive(Component, Debug, Clone, Default, PartialEq, Eq)]
pub struct ItemCooldowns(pub Vec<(&'static str, u64)>);

fn now(world: &World) -> u64 {
    world
        .get_resource::<crate::ecs::resources::TickCounter>()
        .map_or(0, |t| t.current)
}

/// Whether `category` is cooling down for `player`.
pub fn cooling(world: &World, player: Entity, category: &str) -> bool {
    let now = now(world);
    world.get::<ItemCooldowns>(player).is_some_and(|c| {
        c.0.iter()
            .any(|(name, until)| *name == category && now < *until)
    })
}

/// Start a `ticks` cooldown of `category` and tell the player's client.
pub fn start_cooldown(world: &mut World, player: Entity, category: &'static str, ticks: u32) {
    let now = now(world);
    let mut entity = world.entity_mut(player);
    if !entity.contains::<ItemCooldowns>() {
        entity.insert(ItemCooldowns::default());
    }
    let mut cooldowns = entity.get_mut::<ItemCooldowns>().expect("inserted above");
    cooldowns
        .0
        .retain(|(name, until)| *name != category && now < *until);
    cooldowns.0.push((category, now + u64::from(ticks)));
    if let Some(session) = world.get::<PlayerSession>(player) {
        let _ = session.send(jolyne::valentine::McpePacket::from(
            jolyne::valentine::PlayerStartItemCooldownPacket {
                item_category: category.into(),
                duration_ticks: ticks as i32,
            },
        ));
    }
}
