//! Air supply, fire and damage-immunity timers of living entities (status
//! effects are `entity::effects`).

use bevy_ecs::prelude::*;

use crate::entity::components::*;

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

/// Living entities other than mobs (mobs burn through `mob::combat::fire`).
type NonMobLiving = (With<Living>, Without<crate::entity::mob::MobKind>);

/// System: Tick fire duration.
pub fn tick_fire(mut query: Query<(&mut OnFire, &mut Health), NonMobLiving>) {
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
