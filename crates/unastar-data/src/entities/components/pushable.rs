use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:pushable`. Defines what can push an entity between other entities and pistons.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct Pushable {
    /// is_pushable
    pub is_pushable: Option<bool>,
    /// is_pushable_by_piston
    pub is_pushable_by_piston: Option<bool>,
}
impl Default for Pushable {
    fn default() -> Self {
        Self {
            is_pushable: Some(true),
            is_pushable_by_piston: Some(true),
        }
    }
}
