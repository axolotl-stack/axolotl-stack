use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:attack`
#[derive(Component, Debug, Clone, PartialEq)]
pub struct Attack {
    /// damage
    pub damage: i32,
    /// effect_name
    pub effect_name: Option<String>,
    /// effect_duration
    pub effect_duration: Option<f32>,
}
impl Default for Attack {
    fn default() -> Self {
        Self {
            damage: 2i32,
            effect_name: None,
            effect_duration: None,
        }
    }
}
