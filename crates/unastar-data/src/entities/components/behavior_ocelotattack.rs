use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorOcelotattackPriority {}
impl Default for BehaviorOcelotattackPriority {
    fn default() -> Self {
        Self {}
    }
}
/// Bedrock component `minecraft:behavior.ocelotattack`. Can only be used by the Ocelot. Allows it to perform the sneak and pounce attack.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorOcelotattack {
    ///Time (in seconds) between attacks.
    pub cooldown_time: Option<f32>,
    ///Max distance from the target, this entity will use this attack behavior.
    pub max_distance: Option<f32>,
    ///Max distance from the target, this entity starts sneaking.
    pub max_sneak_range: Option<f32>,
    ///Max distance from the target, this entity starts sprinting (sprinting takes priority over sneaking).
    pub max_sprint_range: Option<f32>,
    ///priority
    pub priority: Option<BehaviorOcelotattackPriority>,
    ///Used with the base size of the entity to determine minimum target-distance before trying to deal attack damage.
    pub reach_multiplier: Option<f32>,
    ///Modifies the attacking entity's movement speed while sneaking.
    pub sneak_speed_multiplier: Option<f32>,
    ///Modifies the attacking entity's movement speed while sprinting.
    pub sprint_speed_multiplier: Option<f32>,
    ///Modifies the attacking entity's movement speed when not sneaking or sprinting, but still within attack range.
    pub walk_speed_multiplier: Option<f32>,
    ///Maximum rotation (in degrees), on the X-axis, this entity can rotate while trying to look at the target.
    pub x_max_rotation: Option<f32>,
    ///Maximum rotation (in degrees), on the Y-axis, this entity can rotate its head while trying to look at the target.
    pub y_max_head_rotation: Option<f32>,
}
impl Default for BehaviorOcelotattack {
    fn default() -> Self {
        Self {
            cooldown_time: Some(1f32),
            max_distance: Some(15f32),
            max_sneak_range: Some(15f32),
            max_sprint_range: Some(4f32),
            priority: None,
            reach_multiplier: Some(2f32),
            sneak_speed_multiplier: Some(0.6f32),
            sprint_speed_multiplier: Some(1.33f32),
            walk_speed_multiplier: Some(0.8f32),
            x_max_rotation: Some(30f32),
            y_max_head_rotation: Some(30f32),
        }
    }
}
