//! Effect systems for status effects.

use bevy_ecs::prelude::*;

use crate::entity::components::*;

/// System: Tick all status effects.
pub fn tick_effects(mut query: Query<&mut Effects>) {
    for mut effects in query.iter_mut() {
        effects.tick();
    }
}

/// System: Apply regeneration effect healing.
pub fn apply_regeneration(mut query: Query<(&mut Health, &Effects), With<Living>>) {
    for (mut health, effects) in query.iter_mut() {
        if let Some(regen) = effects.get(EffectType::REGENERATION) {
            // Regeneration ticks every 50 / (level + 1) ticks
            // Level 1 = every 25 ticks, Level 2 = every 12.5 ticks, etc.
            let interval = 50 / (regen.level as u32 + 1);
            if regen.duration_ticks % interval == 0 {
                health.heal(1.0);
            }
        }
    }
}

/// System: Apply poison effect damage.
pub fn apply_poison(mut query: Query<(&mut Health, &Effects), With<Living>>) {
    for (mut health, effects) in query.iter_mut() {
        if let Some(poison) = effects.get(EffectType::POISON) {
            // Poison ticks every 25 / (level + 1) ticks, but doesn't kill
            let interval = 25 / (poison.level as u32 + 1);
            if poison.duration_ticks % interval == 0 && health.current > 1.0 {
                let new_health = (health.current - 1.0).max(1.0);
                health.current = new_health;
            }
        }
    }
}

/// System: Apply wither effect damage.
pub fn apply_wither(mut query: Query<(&mut Health, &Effects), With<Living>>) {
    for (mut health, effects) in query.iter_mut() {
        if let Some(wither) = effects.get(EffectType::WITHER) {
            // Wither ticks every 40 / (level + 1) ticks and can kill
            let interval = 40 / (wither.level as u32 + 1);
            if wither.duration_ticks % interval == 0 {
                health.damage(1.0);
            }
        }
    }
}

/// System: Tick air supply for entities underwater.
pub fn tick_air_supply(mut query: Query<(&Position, &mut AirSupply, &mut Health), With<Living>>) {
    // TODO: Check if head is in water block
    // For now, assume not underwater
    for (_position, mut air_supply, mut _health) in query.iter_mut() {
        let is_underwater = false; // TODO: Block check
        let drowning = air_supply.tick(is_underwater);
        if drowning {
            // TODO: Apply drowning damage
        }
    }
}

/// System: Tick fire duration.
pub fn tick_fire(mut query: Query<(&mut OnFire, &mut Health), With<Living>>) {
    for (mut on_fire, mut health) in query.iter_mut() {
        if on_fire.is_on_fire() {
            on_fire.tick();
            // Fire damage every 20 ticks
            if on_fire.ticks_remaining % 20 == 0 {
                health.damage(1.0);
            }
        }
    }
}

/// System: Tick damage immunity.
pub fn tick_damage_immunity(mut query: Query<&mut DamageImmunity>) {
    for mut immunity in query.iter_mut() {
        immunity.tick();
    }
}
