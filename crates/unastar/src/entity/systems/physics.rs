//! Physics systems for entity movement.

use bevy_ecs::prelude::*;
use glam::DVec3;

use crate::entity::components::*;

/// System: Apply gravity to entities with velocity.
pub fn apply_gravity(
    mut query: Query<(&mut Velocity, Option<&OnGround>, Option<&ProjectileData>)>,
) {
    const GRAVITY: f64 = 0.08;

    for (mut velocity, on_ground, projectile) in query.iter_mut() {
        if on_ground.is_some_and(|on_ground| on_ground.0) {
            continue;
        }
        let gravity = projectile.map_or(GRAVITY, |projectile| projectile.gravity);
        velocity.0.y -= gravity;
    }
}

/// System: Apply velocity to position.
pub fn apply_velocity(mut query: Query<(&mut Position, &Velocity)>) {
    for (mut position, velocity) in query.iter_mut() {
        position.0 += velocity.0;
    }
}

/// System: Apply drag to velocity.
pub fn apply_drag(mut query: Query<(&mut Velocity, Option<&ProjectileData>)>) {
    const DRAG: f64 = 0.02;

    for (mut velocity, projectile) in query.iter_mut() {
        let drag = projectile.map_or(DRAG, |projectile| projectile.drag);
        velocity.0 *= 1.0 - drag;
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

#[cfg(test)]
mod tests {
    use super::*;
    use bevy_ecs::schedule::Schedule;
    use bevy_ecs::world::World;

    #[test]
    fn gravity_applies_to_living_entities_that_are_not_grounded() {
        let mut world = World::new();
        let entity = world
            .spawn((Living, Velocity(DVec3::ZERO), OnGround(false)))
            .id();
        let mut schedule = Schedule::default();
        schedule.add_systems(apply_gravity);

        schedule.run(&mut world);

        let velocity = world.get::<Velocity>(entity).expect("velocity");
        assert_eq!(velocity.0.y, -0.08);
    }

    #[test]
    fn gravity_skips_grounded_living_entities() {
        let mut world = World::new();
        let entity = world
            .spawn((Living, Velocity(DVec3::ZERO), OnGround(true)))
            .id();
        let mut schedule = Schedule::default();
        schedule.add_systems(apply_gravity);

        schedule.run(&mut world);

        let velocity = world.get::<Velocity>(entity).expect("velocity");
        assert_eq!(velocity.0.y, 0.0);
    }

    #[test]
    fn gravity_applies_to_living_entities_without_ground_state() {
        let mut world = World::new();
        let entity = world.spawn((Living, Velocity(DVec3::ZERO))).id();
        let mut schedule = Schedule::default();
        schedule.add_systems(apply_gravity);

        schedule.run(&mut world);

        let velocity = world.get::<Velocity>(entity).expect("velocity");
        assert_eq!(velocity.0.y, -0.08);
    }

    #[test]
    fn gravity_applies_to_dropped_items() {
        let mut world = World::new();
        let entity = world.spawn((DroppedItem, Velocity(DVec3::ZERO))).id();
        let mut schedule = Schedule::default();
        schedule.add_systems(apply_gravity);

        schedule.run(&mut world);

        let velocity = world.get::<Velocity>(entity).expect("velocity");
        assert_eq!(velocity.0.y, -0.08);
    }

    #[test]
    fn projectiles_use_projectile_gravity() {
        let mut world = World::new();
        let snowball = world
            .spawn((
                Projectile,
                ProjectileData::snowball(),
                Velocity(DVec3::ZERO),
            ))
            .id();
        let fireball = world
            .spawn((
                Projectile,
                ProjectileData::fireball(),
                Velocity(DVec3::ZERO),
            ))
            .id();
        let mut schedule = Schedule::default();
        schedule.add_systems(apply_gravity);

        schedule.run(&mut world);

        assert_eq!(
            world.get::<Velocity>(snowball).expect("snowball").0.y,
            -0.03
        );
        assert_eq!(world.get::<Velocity>(fireball).expect("fireball").0.y, 0.0);
    }

    #[test]
    fn projectiles_use_projectile_drag() {
        let mut world = World::new();
        let arrow = world
            .spawn((
                Projectile,
                ProjectileData::arrow(),
                Velocity(DVec3::new(10.0, 0.0, 0.0)),
            ))
            .id();
        let item = world
            .spawn((DroppedItem, Velocity(DVec3::new(10.0, 0.0, 0.0))))
            .id();
        let mut schedule = Schedule::default();
        schedule.add_systems(apply_drag);

        schedule.run(&mut world);

        assert_eq!(world.get::<Velocity>(arrow).expect("arrow").0.x, 9.9);
        assert_eq!(world.get::<Velocity>(item).expect("item").0.x, 9.8);
    }
}
