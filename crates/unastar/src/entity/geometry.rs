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
