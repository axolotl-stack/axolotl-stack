use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:follow_range`
#[derive(Component, Debug, Clone, PartialEq)]
pub struct FollowRange {
    /// value
    pub range: i32,
}
impl Default for FollowRange {
    fn default() -> Self {
        Self { range: 16i32 }
    }
}
