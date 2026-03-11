use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorCircleAroundAnchorPriority {}
impl Default for BehaviorCircleAroundAnchorPriority {
    fn default() -> Self {
        Self {}
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorCircleAroundAnchorSpeedMultiplier {}
impl Default for BehaviorCircleAroundAnchorSpeedMultiplier {
    fn default() -> Self {
        Self {}
    }
}
/// Bedrock component `minecraft:behavior.circle_around_anchor`. Causes an entity to circle around an anchor point placed near a point or target.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorCircleAroundAnchor {
    ///Number of degrees to change this entity's facing by, when the entity selects its next anchor point.
    pub angle_change: Option<f32>,
    ///Maximum distance from the anchor-point in which this entity considers itself to have reached the anchor point. This is to prevent the entity from bouncing back and forth trying to reach a specific spot.
    pub goal_radius: Option<f32>,
    ///The number of blocks above the target that the next anchor point can be set. This value is used only when the entity is tracking a target.
    pub height_above_target_range: Option<crate::types::RangeOrVal<f32>>,
    ///Percent chance to determine how often to increase or decrease the current height around the anchor point. 1 = 100%. `height_change_chance` is deprecated and has been replaced with `height_adjustment_chance`.
    pub height_adjustment_chance: Option<f32>,
    ///A random value to determine when to change the height of the mob from the anchor point. This has a 1/value chance every tick to do so.
    pub height_change_chance: Option<i32>,
    ///The range of height in blocks offset the mob can have from it's anchor point.
    pub height_offset_range: Option<crate::types::RangeOrVal<f32>>,
    ///priority
    pub priority: Option<BehaviorCircleAroundAnchorPriority>,
    ///Percent chance to determine how often to increase the size of the current movement radius around the anchor point. 1 = 100%. `radius_change_chance` is deprecated and has been replaced with `radius_adjustment_chance`.
    pub radius_adjustment_chance: Option<f32>,
    ///The number of blocks to increase the current movement radius by, upon successful `radius_adjustment_chance`. If the current radius increases over the range maximum, the current radius will be set back to the range minimum and the entity will change between clockwise and counter-clockwise movement.
    pub radius_change: Option<f32>,
    ///A random value to determine when to increase the size of the radius up to the maximum. This has a 1/value chance every tick to do so.
    pub radius_change_chance: Option<i32>,
    ///Horizontal distance from the anchor point this entity must stay within upon a successful radius adjustment.
    pub radius_range: Option<crate::types::RangeOrVal<f32>>,
    ///speed_multiplier
    pub speed_multiplier: Option<BehaviorCircleAroundAnchorSpeedMultiplier>,
}
impl Default for BehaviorCircleAroundAnchor {
    fn default() -> Self {
        Self {
            angle_change: Some(15f32),
            goal_radius: Some(0.5f32),
            height_above_target_range: Some(crate::types::RangeOrVal::Range {
                min: 0f32,
                max: 0f32,
            }),
            height_adjustment_chance: Some(0.002857f32),
            height_change_chance: None,
            height_offset_range: Some(crate::types::RangeOrVal::Range {
                min: 0f32,
                max: 0f32,
            }),
            priority: Some(BehaviorCircleAroundAnchorPriority {}),
            radius_adjustment_chance: Some(0.004f32),
            radius_change: Some(1f32),
            radius_change_chance: None,
            radius_range: Some(crate::types::RangeOrVal::Range {
                min: 5f32,
                max: 15f32,
            }),
            speed_multiplier: Some(BehaviorCircleAroundAnchorSpeedMultiplier {}),
        }
    }
}
