use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:burns_in_daylight`. Specifies if/how a mob burns in daylight.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BurnsInDaylight {
    ///value
    pub value: crate::types::BedrockValue,
}
impl Default for BurnsInDaylight {
    fn default() -> Self {
        Self {
            value: crate::types::BedrockValue::Null,
        }
    }
}
