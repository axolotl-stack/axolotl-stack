//! Closed bodies for timed anger and gaze detection.

use serde::Deserialize;

use super::{EventTrigger, FilterJson};
use crate::actor_components::body::FloatRange;

/// `minecraft:angry`: a timed state and its calm event.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Angry {
    #[serde(default = "anger_duration")]
    pub duration: f32,
    #[serde(default)]
    pub duration_delta: f32,
    pub calm_event: Option<EventTrigger>,
    pub angry_sound: Option<String>,
    #[serde(default = "zero_range")]
    pub sound_interval: FloatRange,
    pub filters: Option<FilterJson>,
    #[serde(default)]
    pub broadcast_anger: bool,
    #[serde(default)]
    pub broadcast_anger_on_attack: bool,
    #[serde(default)]
    pub broadcast_anger_on_being_attacked: bool,
    #[serde(default = "yes")]
    pub broadcast_anger_when_dying: bool,
    pub broadcast_filters: Option<FilterJson>,
    #[serde(default = "broadcast_range")]
    pub broadcast_range: f32,
    #[serde(default)]
    pub broadcast_targets: Vec<String>,
}

/// The published default anger duration, in seconds.
fn anger_duration() -> f32 {
    25.0
}
/// The published default broadcast distance, in blocks.
fn broadcast_range() -> f32 {
    20.0
}
/// The shared true default.
fn yes() -> bool {
    true
}
/// An omitted interval has no delay.
fn zero_range() -> FloatRange {
    FloatRange { min: 0.0, max: 0.0 }
}

/// How gaze detection assigns its combat target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LookTargetMode {
    Never,
    OnceAndStopScanning,
    OnceAndKeepScanning,
}

/// Current string choices and the legacy Boolean targeting form.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(untagged)]
pub enum LookTarget {
    Mode(LookTargetMode),
    Legacy(bool),
}

/// An omitted target choice never assigns a target.
fn never() -> LookTarget {
    LookTarget::Mode(LookTargetMode::Never)
}

/// A body-relative point for a line-of-sight check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LookLocationName {
    Head,
    Body,
    Feet,
}

/// A named point and optional vertical offset, in blocks.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LookLocationObject {
    pub location: LookLocationName,
    #[serde(default)]
    pub vertical_offset: f32,
}

/// A location accepts the simple name and the configured offset form.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(untagged)]
pub enum LookLocation {
    Name(LookLocationName),
    Object(LookLocationObject),
}

/// `minecraft:looked_at`: detecting a continuous view of an actor.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LookedAt {
    #[serde(default = "gaze_fov")]
    pub field_of_view: f32,
    pub filters: Option<FilterJson>,
    #[serde(default)]
    pub find_players_only: bool,
    #[serde(default = "collision")]
    pub line_of_sight_obstruction_type: String,
    #[serde(default)]
    pub look_at_locations: Vec<LookLocation>,
    #[serde(default = "zero_range")]
    pub looked_at_cooldown: FloatRange,
    pub looked_at_event: Option<EventTrigger>,
    #[serde(default)]
    pub min_looked_at_duration: f32,
    pub not_looked_at_event: Option<EventTrigger>,
    #[serde(default = "yes")]
    pub scale_fov_by_distance: bool,
    #[serde(default = "gaze_radius")]
    pub search_radius: f32,
    #[serde(default = "never")]
    pub set_target: LookTarget,
}

/// The published full view cone in degrees at one block's distance.
fn gaze_fov() -> f32 {
    26.0
}
/// The published default search radius, in blocks.
fn gaze_radius() -> f32 {
    10.0
}
/// Collision geometry is the default obstruction mode.
fn collision() -> String {
    "collision".into()
}
