//! Positions and look directions of entities, for any system that needs
//! them (commands, AI, interactions).
//!
//! A player's `Position` is its eye position (what the client reports);
//! every other actor's is its feet. These helpers hide that difference.

use crate::entity::PLAYER_EYE_HEIGHT;
use crate::entity::components::Player;
use crate::entity::components::transform::Position;
use bevy_ecs::prelude::{Entity, World};
use glam::DVec3;

/// Height of the eyes above the feet (players only; other actors use their feet).
pub fn eye_height(world: &World, entity: Entity) -> f64 {
    if world.get::<Player>(entity).is_some() {
        PLAYER_EYE_HEIGHT
    } else {
        0.0
    }
}

/// Feet position of an entity.
pub fn feet_position(world: &World, entity: Entity) -> Option<DVec3> {
    let position = world.get::<Position>(entity)?.0;
    Some(position - DVec3::Y * eye_height(world, entity))
}

/// Eye position of an entity.
pub fn eye_position(world: &World, entity: Entity) -> Option<DVec3> {
    feet_position(world, entity).map(|feet| feet + DVec3::Y * eye_height(world, entity))
}

/// Authoritative collision bounds for living bodies, players and projectiles.
/// Projectiles expose geometry without acquiring living-physics components.
pub fn collision_box(world: &World, entity: Entity) -> Option<crate::registry::collision::Aabb> {
    use crate::entity::components::PhysicsBody;
    use crate::entity::definition::components::{RawCollisionBox, ScaleConfig};
    use crate::registry::collision::Aabb;
    if world.get::<Player>(entity).is_some() {
        let eye = world.get::<Position>(entity)?.0;
        return eye
            .is_finite()
            .then(|| crate::server::game::player_box(eye));
    }
    let feet = feet_position(world, entity)?;
    if !feet.is_finite() {
        return None;
    }
    if let Some(body) = world.get::<PhysicsBody>(entity) {
        return (body.width.is_finite()
            && body.height.is_finite()
            && body.width >= 0.0
            && body.height >= 0.0)
            .then(|| Aabb::from_feet(feet, body.width, body.height));
    }
    let raw = world.get::<RawCollisionBox>(entity)?;
    let scale = world
        .get::<ScaleConfig>(entity)
        .map_or(1.0, |scale| scale.value);
    if !raw.width.is_finite() || !raw.height.is_finite() || !scale.is_finite() || scale < 0.0 {
        return None;
    }
    Some(Aabb::from_feet(
        feet,
        f64::from(raw.width.max(0.0) * scale),
        f64::from(raw.height.max(0.0) * scale),
    ))
}

/// Yaw (degrees, Bedrock convention: 0 faces +z, 90 faces −x) from `from` towards `to`.
pub fn yaw_towards(from: DVec3, to: DVec3) -> f32 {
    let delta = to - from;
    (-delta.x.atan2(delta.z)).to_degrees() as f32
}

/// Yaw and pitch (degrees; positive pitch looks down) that look from `eye` at `at`.
pub fn look_at(eye: DVec3, at: DVec3) -> (f32, f32) {
    let delta = at - eye;
    let horizontal = (delta.x * delta.x + delta.z * delta.z).sqrt();
    (
        yaw_towards(eye, at),
        (-delta.y.atan2(horizontal)).to_degrees() as f32,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn look_angles_follow_the_bedrock_convention() {
        let eye = DVec3::ZERO;
        assert_eq!(yaw_towards(eye, DVec3::Z), 0.0);
        assert!((yaw_towards(eye, -DVec3::X) - 90.0).abs() < 1e-4);
        let (_, pitch) = look_at(eye, DVec3::new(0.0, -1.0, 1.0));
        assert!((pitch - 45.0).abs() < 1e-4);
    }
}
