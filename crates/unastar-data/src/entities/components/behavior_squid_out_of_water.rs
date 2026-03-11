use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorSquidOutOfWaterPriority {}
impl Default for BehaviorSquidOutOfWaterPriority {
    fn default() -> Self {
        Self {}
    }
}
/// Bedrock component `minecraft:behavior.squid_out_of_water`. Allows the squid to stick to the ground when outside water. Can only be used by the Squid.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorSquidOutOfWater {
    ///priority
    pub priority: Option<BehaviorSquidOutOfWaterPriority>,
}
impl Default for BehaviorSquidOutOfWater {
    fn default() -> Self {
        Self { priority: None }
    }
}
