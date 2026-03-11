use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorRandomHoverPriority {}
impl Default for BehaviorRandomHoverPriority {
    fn default() -> Self {
        Self {}
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorRandomHoverSpeedMultiplier {}
impl Default for BehaviorRandomHoverSpeedMultiplier {
    fn default() -> Self {
        Self {}
    }
}
/// Bedrock component `minecraft:behavior.random_hover`. Allows the mob to hover around randomly, close to the surface.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorRandomHover {
    ///The height above the surface which the mob will try to maintain.
    pub hover_height: Option<Vec<f32>>,
    ///A random value to determine when to randomly move somewhere. This has a 1/interval chance to choose this goal
    pub interval: Option<i32>,
    ///priority
    pub priority: Option<BehaviorRandomHoverPriority>,
    ///speed_multiplier
    pub speed_multiplier: Option<BehaviorRandomHoverSpeedMultiplier>,
    ///Distance in blocks on ground that the mob will look for a new spot to move to. Must be at least 1
    pub xz_dist: Option<i32>,
    ///Distance in blocks that the mob will look up or down for a new spot to move to. Must be at least 1
    pub y_dist: Option<i32>,
    ///Height in blocks to add to the selected target position.
    pub y_offset: Option<f32>,
}
impl Default for BehaviorRandomHover {
    fn default() -> Self {
        Self {
            hover_height: None,
            interval: Some(120i32),
            priority: None,
            speed_multiplier: Some(BehaviorRandomHoverSpeedMultiplier {}),
            xz_dist: Some(10i32),
            y_dist: Some(7i32),
            y_offset: Some(0f32),
        }
    }
}
