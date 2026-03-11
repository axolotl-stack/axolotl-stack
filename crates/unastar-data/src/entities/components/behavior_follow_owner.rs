use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorFollowOwnerPriority {}
impl Default for BehaviorFollowOwnerPriority {
    fn default() -> Self {
        Self {}
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorFollowOwnerSpeedMultiplier {}
impl Default for BehaviorFollowOwnerSpeedMultiplier {
    fn default() -> Self {
        Self {}
    }
}
/// Bedrock component `minecraft:behavior.follow_owner`. Allows the mob to follow the player that owns them.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorFollowOwner {
    ///Specify if the mob can teleport to the player if it is too far away.
    pub can_teleport: Option<bool>,
    ///Specify if the mob will follow the owner if it has heard a vibration lately.
    pub ignore_vibration: Option<bool>,
    ///The maximum distance in blocks this mob can be from its owner to start following, only used when canTeleport is false.
    pub max_distance: Option<f32>,
    ///Defines how far (in blocks) the entity will be from its owner after teleporting. If not specified, it defaults to "stop_distance" + 1, allowing the entity to seamlessly resume navigation.
    pub post_teleport_distance: Option<f32>,
    ///priority
    pub priority: Option<BehaviorFollowOwnerPriority>,
    ///speed_multiplier
    pub speed_multiplier: Option<BehaviorFollowOwnerSpeedMultiplier>,
    ///The distance in blocks that the owner can be away from this mob before it starts following it.
    pub start_distance: Option<f32>,
    ///The distance in blocks this mob will stop from its owner while following it.
    pub stop_distance: Option<f32>,
}
impl Default for BehaviorFollowOwner {
    fn default() -> Self {
        Self {
            can_teleport: Some(true),
            ignore_vibration: Some(true),
            max_distance: Some(60f32),
            post_teleport_distance: Some(0f32),
            priority: None,
            speed_multiplier: Some(BehaviorFollowOwnerSpeedMultiplier {}),
            start_distance: Some(10f32),
            stop_distance: Some(2f32),
        }
    }
}
