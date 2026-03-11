use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:environment_sensor`. Creates a trigger based on environment conditions.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct EnvironmentSensor {
    ///The list of triggers that fire when the environment conditions match the given filter criteria.
    pub triggers: Option<crate::types::BedrockValue>,
}
impl Default for EnvironmentSensor {
    fn default() -> Self {
        Self { triggers: None }
    }
}
