//! Status effect names and the player entry points of the shared effect
//! system (`entity::effects`): food, milk and other item use give, take and
//! clear effects through [`add_effect`], [`remove_effect`] and
//! [`clear_effects`]. The effect step, what effects do to a player and the
//! `MobEffect` packets are `entity::effects`, shared with mobs.

use bevy_ecs::prelude::*;

use crate::entity::components::{ActiveEffect, EffectType, Health, Hunger};

/// Bedrock effect identifiers and their protocol ids.
const EFFECT_NAMES: [(&str, u8); 36] = [
    ("speed", 1),
    ("slowness", 2),
    ("haste", 3),
    ("mining_fatigue", 4),
    ("strength", 5),
    ("instant_health", 6),
    ("instant_damage", 7),
    ("jump_boost", 8),
    ("nausea", 9),
    ("regeneration", 10),
    ("resistance", 11),
    ("fire_resistance", 12),
    ("water_breathing", 13),
    ("invisibility", 14),
    ("blindness", 15),
    ("night_vision", 16),
    ("hunger", 17),
    ("weakness", 18),
    ("poison", 19),
    ("wither", 20),
    ("health_boost", 21),
    ("absorption", 22),
    ("saturation", 23),
    ("levitation", 24),
    ("fatal_poison", 25),
    ("conduit_power", 26),
    ("slow_falling", 27),
    ("bad_omen", 28),
    ("village_hero", 29),
    ("darkness", 30),
    ("trial_omen", 31),
    ("wind_charged", 32),
    ("weaving", 33),
    ("oozing", 34),
    ("infested", 35),
    ("raid_omen", 36),
];

/// The effect named `name` (`minecraft:` prefix optional, case-insensitive
/// as pack JSON is lenient here).
pub fn effect_by_name(name: &str) -> Option<EffectType> {
    let name = name.strip_prefix("minecraft:").unwrap_or(name);
    EFFECT_NAMES
        .iter()
        .find(|(known, _)| known.eq_ignore_ascii_case(name))
        .map(|(_, id)| EffectType(*id))
}

/// Give `player` an effect (amplifier 0-based) through the shared effect
/// system. Instant health and an instant saturation from food act at once
/// and are not stored.
pub fn add_effect(
    world: &mut World,
    player: Entity,
    effect: EffectType,
    amplifier: u8,
    duration_ticks: u32,
) {
    match effect {
        EffectType::INSTANT_HEALTH => {
            heal(world, player, (4u32 << amplifier.min(30)) as f32);
            return;
        }
        EffectType::SATURATION if duration_ticks <= 1 => {
            saturate(world, player, amplifier);
            return;
        }
        _ => {}
    }
    crate::entity::effects::give(
        world,
        player,
        effect,
        ActiveEffect::new(amplifier, duration_ticks),
    );
}

/// Remove one effect from `player`.
pub fn remove_effect(world: &mut World, player: Entity, effect: EffectType) {
    crate::entity::effects::take(world, player, effect);
}

/// Remove every effect (milk).
pub fn clear_effects(world: &mut World, player: Entity) {
    crate::entity::effects::clear(world, player);
}

fn heal(world: &mut World, player: Entity, amount: f32) {
    if let Some(mut health) = world.get_mut::<Health>(player) {
        health.heal(amount);
    }
}

/// Saturation: `amplifier + 1` food points and twice that saturation.
fn saturate(world: &mut World, player: Entity, amplifier: u8) {
    if let Some(mut hunger) = world.get_mut::<Hunger>(player) {
        hunger.eat(i32::from(amplifier) + 1, 1.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effect_names_resolve_to_protocol_ids() {
        assert_eq!(
            effect_by_name("regeneration"),
            Some(EffectType::REGENERATION)
        );
        assert_eq!(
            effect_by_name("minecraft:absorption"),
            Some(EffectType::ABSORPTION)
        );
        assert_eq!(effect_by_name("fatal_poison"), Some(EffectType(25)));
        assert_eq!(effect_by_name("not_an_effect"), None);
    }
}
