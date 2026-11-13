//! `minecraft:teleport` (run by `mob::life::teleport`).

use bevy_ecs::prelude::*;
use glam::DVec3;
use thiserror::Error;
use unastar_data::actor_components::body::{self, Vec3};

use super::common::insert;
use crate::entity::definition::{ComponentError, ComponentKindsBuilder, DefinitionError};

/// `minecraft:teleport`: random teleports, teleports when hurt and when a
/// projectile would hit, and teleports toward a far target.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct TeleportConfig {
    /// Teleport at random (the timer and the light and dark rolls).
    pub random_teleports: bool,
    /// The per-tick roll in daylight.
    pub light_chance: f32,
    /// The per-tick roll in the dark.
    pub dark_chance: f32,
    /// The random teleport timer's bounds, in ticks.
    pub min_random_ticks: u32,
    pub max_random_ticks: u32,
    /// Half the random teleport cube: a target is drawn up to this far
    /// from the feet on each axis.
    pub reach: DVec3,
    /// Teleport toward a target farther than this.
    pub target_distance: f32,
    pub target_chance: f32,
    /// A projectile that would hit makes the mob teleport instead.
    pub on_projectile_hit: bool,
}

/// A `minecraft:teleport` body the runtime cannot honour.
#[derive(Debug, Error)]
pub enum TeleportError {
    #[error("teleport: min_random_teleport_time {min} is above max_random_teleport_time {max}")]
    TimeBounds { min: f32, max: f32 },
    #[error("teleport: a negative random_teleport_cube")]
    NegativeCube,
}

impl ComponentError for TeleportError {}

fn ticks(seconds: f32) -> u32 {
    (seconds.max(0.0) * 20.0).round() as u32
}

/// Register the teleport component kind.
pub fn register_component_kinds(
    builder: &mut ComponentKindsBuilder,
) -> Result<(), DefinitionError> {
    insert(builder, |b: body::Teleport| {
        if b.min_random_teleport_time > b.max_random_teleport_time {
            return Err(TeleportError::TimeBounds {
                min: b.min_random_teleport_time,
                max: b.max_random_teleport_time,
            });
        }
        let cube = match b.random_teleport_cube {
            Vec3::Array([x, y, z]) => DVec3::new(f64::from(x), f64::from(y), f64::from(z)),
            Vec3::Vec3Object(v) => DVec3::new(f64::from(v.x), f64::from(v.y), f64::from(v.z)),
        };
        if cube.min_element() < 0.0 {
            return Err(TeleportError::NegativeCube);
        }
        Ok(TeleportConfig {
            random_teleports: b.random_teleports,
            light_chance: b.light_teleport_chance,
            dark_chance: b.dark_teleport_chance,
            min_random_ticks: ticks(b.min_random_teleport_time),
            max_random_ticks: ticks(b.max_random_teleport_time),
            reach: cube / 2.0,
            target_distance: b.target_distance,
            target_chance: b.target_teleport_chance,
            on_projectile_hit: b.teleports_on_projectile_hit,
        })
    })
}
