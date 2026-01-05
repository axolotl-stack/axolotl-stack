use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:pushable`
#[derive(Component, Debug, Clone, PartialEq)]
pub struct Pushable {
    /// is_pushable
    pub is_pushable: bool,
    /// is_pushable_by_piston
    pub is_pushable_by_piston: bool,
}
impl Default for Pushable {
    fn default() -> Self {
        Self {
            is_pushable: true,
            is_pushable_by_piston: true,
        }
    }
}
