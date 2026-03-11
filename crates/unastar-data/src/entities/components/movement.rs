use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:movement`. Specifies the initial value of a specific attribute for an entity when spawned.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct Movement {
    ///The maximum starting health an entity has.
    pub max: Option<f32>,
    ///The minimum starting health an entity has.
    pub min: Option<f32>,
    ///The amount of health an entity to start with by default.
    pub value: crate::types::RangeOrVal<f32>,
}
impl Default for Movement {
    fn default() -> Self {
        Self {
            max: None,
            min: None,
            value: crate::types::RangeOrVal::Fixed(0.25f32),
        }
    }
}
