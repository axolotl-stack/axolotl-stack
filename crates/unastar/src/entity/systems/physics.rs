//! Physics systems for entity movement.

use bevy_ecs::prelude::*;
use glam::DVec3;

use crate::entity::components::*;

/// System: Apply gravity to entities with velocity.
pub fn apply_gravity(mut query: Query<&mut Velocity, (With<Living>, Without<OnGround>)>) {
    const GRAVITY: f64 = 0.08;

    for mut velocity in query.iter_mut() {
        velocity.0.y -= GRAVITY;
    }
}

/// System: Apply velocity to position.
pub fn apply_velocity(mut query: Query<(&mut Position, &Velocity)>) {
    for (mut position, velocity) in query.iter_mut() {
        position.0 += velocity.0;
    }
}

/// System: Apply drag to velocity.
pub fn apply_drag(mut query: Query<&mut Velocity>) {
    const DRAG: f64 = 0.02;

    for mut velocity in query.iter_mut() {
        velocity.0 *= 1.0 - DRAG;
    }
}

/// System: Clamp velocity to reasonable values.
pub fn clamp_velocity(mut query: Query<&mut Velocity>) {
    const MAX_VELOCITY: f64 = 100.0;

    for mut velocity in query.iter_mut() {
        velocity.0 = velocity
            .0
            .clamp(DVec3::splat(-MAX_VELOCITY), DVec3::splat(MAX_VELOCITY));
    }
}

/// System: Check ground collision (placeholder).
pub fn check_ground_collision(mut query: Query<(&Position, &mut OnGround, &mut Velocity)>) {
    // Simple Y=0 ground check for now
    // TODO: Real block collision detection
    for (position, mut on_ground, mut velocity) in query.iter_mut() {
        if position.0.y <= 0.0 && velocity.0.y <= 0.0 {
            on_ground.0 = true;
            velocity.0.y = 0.0;
        } else if position.0.y > 0.0 {
            on_ground.0 = false;
        }
    }
}

/// System: Apply knockback from damage.
pub fn apply_knockback(
    _query: Query<(&mut Velocity, &Position)>,
    // TODO: Read knockback events
) {
    // Placeholder - would read knockback events and apply to velocity
}
