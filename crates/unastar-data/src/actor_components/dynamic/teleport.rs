//! The published teleport component's closed authored body.

use crate::actor_components::{FilterJson, body::Vec3};
use serde::Deserialize;

/// `minecraft:teleport`: random, target-directed and projectile-reactive hops.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Teleport {
    #[serde(default = "yes")]
    pub random_teleports: bool,
    #[serde(default)]
    pub min_random_teleport_time: f32,
    #[serde(default = "max_time")]
    pub max_random_teleport_time: f32,
    #[serde(default = "cube")]
    pub random_teleport_cube: Vec3,
    #[serde(default = "chance")]
    pub dark_teleport_chance: f32,
    #[serde(default = "chance")]
    pub light_teleport_chance: f32,
    #[serde(default = "target_distance")]
    pub target_distance: f32,
    #[serde(default = "always")]
    pub target_teleport_chance: f32,
    #[serde(default)]
    pub teleports_on_projectile_hit: bool,
    pub projectile_filter: Option<FilterJson>,
}
/// Random teleports are enabled unless explicitly disabled.
fn yes() -> bool {
    true
}
/// The default maximum random wait, in seconds.
fn max_time() -> f32 {
    20.0
}
/// The default random search dimensions, in blocks.
fn cube() -> Vec3 {
    Vec3::Array([32.0, 16.0, 32.0])
}
/// The default ambient chance in either lighting condition.
fn chance() -> f32 {
    0.01
}
/// The default target-directed maximum displacement, in blocks.
fn target_distance() -> f32 {
    16.0
}
/// Target-directed attempts are enabled by default.
fn always() -> f32 {
    1.0
}
