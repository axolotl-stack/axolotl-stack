//! Entity lifecycle systems.

use bevy_ecs::prelude::*;

use crate::entity::components::*;

/// System: Tick entity age.
pub fn tick_age(mut query: Query<&mut Age>) {
    for mut age in query.iter_mut() {
        age.tick();
    }
}

/// System: Despawn dead entities.
pub fn despawn_dead(mut commands: Commands, query: Query<(Entity, &Health), With<Living>>) {
    for (entity, health) in query.iter() {
        if health.is_dead() {
            // TODO: Send death event, play death animation, drop items
            commands.entity(entity).despawn();
        }
    }
}

/// System: Tick item pickup delay.
pub fn tick_item_pickup_delay(mut query: Query<&mut PickupDelay, With<DroppedItem>>) {
    for mut delay in query.iter_mut() {
        delay.tick();
    }
}

/// System: Tick item despawn timer.
pub fn tick_item_despawn(
    mut commands: Commands,
    mut query: Query<(Entity, &mut DespawnTimer), With<DroppedItem>>,
) {
    for (entity, mut timer) in query.iter_mut() {
        timer.tick();
        if timer.should_despawn() {
            commands.entity(entity).despawn();
        }
    }
}

/// System: Tick projectile age and despawn old projectiles.
pub fn tick_projectile_lifetime(
    mut commands: Commands,
    query: Query<(Entity, &Age, &ProjectileHit), With<Projectile>>,
) {
    const MAX_PROJECTILE_AGE: u64 = 1200; // 1 minute
    const MAX_STUCK_TICKS: u32 = 1200; // 1 minute stuck in block

    for (entity, age, hit) in query.iter() {
        // Despawn if too old or stuck too long
        if age.0 > MAX_PROJECTILE_AGE || hit.stuck_ticks > MAX_STUCK_TICKS {
            commands.entity(entity).despawn();
        }
    }
}

/// System: Tick mob age (for baby growth).
pub fn tick_mob_age(mut query: Query<&mut MobAge, With<Mob>>) {
    for mut mob_age in query.iter_mut() {
        mob_age.tick();
    }
}
