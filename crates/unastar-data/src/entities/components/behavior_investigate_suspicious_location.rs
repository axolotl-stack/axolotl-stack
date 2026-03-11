use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorInvestigateSuspiciousLocationControlFlags {}
impl Default for BehaviorInvestigateSuspiciousLocationControlFlags {
    fn default() -> Self {
        Self {}
    }
}
/// Bedrock component `minecraft:behavior.investigate_suspicious_location`. Allows the mob to inspect bookshelves.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorInvestigateSuspiciousLocation {
    ///control_flags
    pub control_flags: Option<BehaviorInvestigateSuspiciousLocationControlFlags>,
    ///Distance in blocks within the entity considers it has reached its target position.
    pub goal_radius: Option<f32>,
    ///The higher the priority, the sooner this behavior will be executed as a goal.
    pub priority: Option<i32>,
    ///Movement speed multiplier.
    pub speed_multiplier: Option<f32>,
}
impl Default for BehaviorInvestigateSuspiciousLocation {
    fn default() -> Self {
        Self {
            control_flags: Some(BehaviorInvestigateSuspiciousLocationControlFlags {}),
            goal_radius: Some(1.5f32),
            priority: Some(0i32),
            speed_multiplier: Some(1f32),
        }
    }
}
