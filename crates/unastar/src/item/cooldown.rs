//! Item cooldowns: `minecraft:cooldown` in the vanilla item documents.
//!
//! Measured on BDS 1.26.51 (the spear capture,
//! `tools/bds-sweep/scripts/spears.txt`, replayed by
//! `entity/mob/combat/hit/spear_tests.rs`):
//!
//! - An item whose cooldown has the type `attack` (the spears, category
//!   `spear`) starts it when the player hits a mob with it, and when its jab
//!   (item use `UseAsAttack`) hits nothing.
//! - Starting it sends the player `PlayerStartItemCooldown` with the category
//!   (`minecraft:` + the component's category) and the duration in ticks
//!   (the component's seconds × 20: 13 to 23 for wooden to netherite).
//! - While it runs, every hit and jab made with an item of that category is
//!   refused: no damage, no sound and no new cooldown. A hit 18 inputs after
//!   the last one is refused, one 19 inputs after it lands.
//! - Items of another category, and items without a cooldown (a sword), are
//!   not held back.

use std::collections::HashMap;

use bevy_ecs::prelude::*;
use jolyne::valentine::{McpePacket, PlayerStartItemCooldownPacket};

use crate::ecs::resources::TickCounter;
use crate::entity::components::PlayerSession;
use crate::registry::item::ItemEntry;

/// An item's cooldown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cooldown {
    /// The category as the client names it (`minecraft:spear`).
    pub category: String,
    pub ticks: u32,
    /// Started by attacking (`type: attack`) rather than by using.
    pub attack: bool,
}

/// The cooldown of an item, from its `minecraft:cooldown` component.
pub fn cooldown(entry: &ItemEntry) -> Option<Cooldown> {
    let component = entry.components().cooldown.as_ref()?;
    let category = if component.category.contains(':') {
        component.category.clone()
    } else {
        format!("minecraft:{}", component.category)
    };
    Some(Cooldown {
        category,
        // 0.95 s is 18.999998 ticks in f32: round, as BDS sends 19.
        ticks: (component.duration * 20.0).round() as u32,
        attack: component.kind.as_deref() == Some("attack"),
    })
}

/// The cooldowns a player has started: category to (start tick, ticks).
#[derive(Component, Debug, Default, Clone)]
pub struct ItemCooldowns(HashMap<String, (u64, u32)>);

fn now(world: &World) -> u64 {
    world
        .get_resource::<TickCounter>()
        .map_or(0, |tick| tick.current)
}

/// Whether `category` is cooling down for `player`.
pub fn cooling(world: &World, player: Entity, category: &str) -> bool {
    let now = now(world);
    world
        .get::<ItemCooldowns>(player)
        .and_then(|cooldowns| cooldowns.0.get(category))
        .is_some_and(|&(start, ticks)| now < start + u64::from(ticks))
}

/// Start `cooldown` for `player` and tell its client.
pub fn start(world: &mut World, player: Entity, cooldown: &Cooldown) {
    let now = now(world);
    let mut entity = world.entity_mut(player);
    match entity.get_mut::<ItemCooldowns>() {
        Some(mut cooldowns) => {
            cooldowns
                .0
                .insert(cooldown.category.clone(), (now, cooldown.ticks));
        }
        None => {
            let mut cooldowns = ItemCooldowns::default();
            cooldowns
                .0
                .insert(cooldown.category.clone(), (now, cooldown.ticks));
            entity.insert(cooldowns);
        }
    }
    if let Some(session) = world.get::<PlayerSession>(player) {
        let _ = session.send(McpePacket::from(PlayerStartItemCooldownPacket {
            item_category: cooldown.category.as_str().into(),
            duration_ticks: cooldown.ticks as i32,
        }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::item::test_support::stack;

    fn of(item: &str) -> Option<Cooldown> {
        stack(item, 1).item().and_then(cooldown)
    }

    #[test]
    fn spear_cooldowns_come_from_the_item_documents() {
        let ticks: Vec<u32> = [
            "wooden",
            "stone",
            "copper",
            "iron",
            "golden",
            "diamond",
            "netherite",
        ]
        .iter()
        .map(|tier| of(&format!("minecraft:{tier}_spear")).unwrap().ticks)
        .collect();
        assert_eq!(ticks, [13, 15, 17, 19, 19, 21, 23]);
        let iron = of("minecraft:iron_spear").unwrap();
        assert_eq!(iron.category, "minecraft:spear");
        assert!(iron.attack);
        let charge = of("minecraft:wind_charge").unwrap();
        assert_eq!(
            (charge.category.as_str(), charge.ticks),
            ("minecraft:wind_charge", 10)
        );
        assert!(!charge.attack);
        assert_eq!(of("minecraft:iron_sword"), None);
    }
}
