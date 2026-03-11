use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorFloatWanderPriority {}
impl Default for BehaviorFloatWanderPriority {
    fn default() -> Self {
        Self {}
    }
}
/// Bedrock component `minecraft:behavior.float_wander`. Allows the mob to float around like the Ghast.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorFloatWander {
    ///If true, the mob will have an additional buffer zone around it to avoid collisions with blocks when picking a position to wander to.
    pub additional_collision_buffer: Option<bool>,
    ///If true allows the mob to navigate through liquids on its way to the target position.
    pub allow_navigating_through_liquids: Option<bool>,
    ///Range of time in seconds the mob will float around before landing and choosing to do something else.
    pub float_duration: Option<crate::types::RangeOrVal<f32>>,
    ///If true, the MoveControl flag will be added to the behavior which means that it can no longer be active at the same time as other behaviors with MoveControl.
    pub float_wander_has_move_control: Option<bool>,
    ///If true, the point has to be reachable to be a valid target.
    pub must_reach: Option<bool>,
    ///If true, will prioritize finding random positions in the vicinity of surfaces, i.e. blocks that are not Air or Liquid.
    pub navigate_around_surface: Option<bool>,
    ///priority
    pub priority: Option<BehaviorFloatWanderPriority>,
    ///If true, the mob will randomly pick a new point while moving to the previously selected one.
    pub random_reselect: Option<bool>,
    ///The horizontal distance in blocks that the goal will check for a surface from a candidate position. Only valid when `navigate_around_surface` is true.
    pub surface_xz_dist: Option<i32>,
    ///The vertical distance in blocks that the goal will check for a surface from a candidate position. Only valid when `navigate_around_surface` is true.
    pub surface_y_dist: Option<i32>,
    ///If true, the mob will respect home position restrictions when choosing new target positions. If false, it will choose target position without considering home restrictions.
    pub use_home_position_restriction: Option<bool>,
    ///Distance in blocks on ground that the mob will look for a new spot to move to. Must be at least 1
    pub xz_dist: Option<i32>,
    ///Distance in blocks that the mob will look up or down for a new spot to move to. Must be at least 1
    pub y_dist: Option<i32>,
    ///Height in blocks to add to the selected target position.
    pub y_offset: Option<f32>,
}
impl Default for BehaviorFloatWander {
    fn default() -> Self {
        Self {
            additional_collision_buffer: Some(false),
            allow_navigating_through_liquids: Some(false),
            float_duration: None,
            float_wander_has_move_control: Some(true),
            must_reach: Some(false),
            navigate_around_surface: Some(false),
            priority: None,
            random_reselect: Some(false),
            surface_xz_dist: Some(0i32),
            surface_y_dist: Some(0i32),
            use_home_position_restriction: Some(true),
            xz_dist: Some(10i32),
            y_dist: Some(7i32),
            y_offset: Some(0f32),
        }
    }
}
