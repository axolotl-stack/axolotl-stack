use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:health`
#[derive(Component, Debug, Clone, PartialEq)]
pub struct Health {
    /// value
    pub value: i32,
    /// max
    pub max: Option<i32>,
}
impl Default for Health {
    fn default() -> Self {
        Self {
            value: 20i32,
            max: None,
        }
    }
}
