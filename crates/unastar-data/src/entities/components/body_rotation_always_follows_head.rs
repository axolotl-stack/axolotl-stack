use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:body_rotation_always_follows_head`. Causes the entity's body to always be automatically rotated to align with the entity's head. Does not override the "minecraft:body_rotation_blocked" component.
#[derive(Component, Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[component(storage = "SparseSet")]
pub struct BodyRotationAlwaysFollowsHead;
