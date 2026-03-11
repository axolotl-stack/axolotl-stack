use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct BlockSensorOnBreak {
    ///List of blocks that will trigger the sensor.
    pub block_list: Option<Vec<String>>,
    ///Event to run when a block breaks.
    pub on_block_broken: Option<String>,
}
impl Default for BlockSensorOnBreak {
    fn default() -> Self {
        Self {
            block_list: None,
            on_block_broken: None,
        }
    }
}
/// Bedrock component `minecraft:block_sensor`. Fires off a specified event when a block in the block list is broken within the sensor range.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BlockSensor {
    ///Blocks that will trigger the component when broken and what event will trigger.
    pub on_break: Option<Vec<BlockSensorOnBreak>>,
    ///The maximum radial distance in which a specified block can be detected. The biggest radius is 32.0.
    pub sensor_radius: Option<i32>,
    ///List of sources that break the block to listen for. If none are specified, all block breaks will be detected.
    pub sources: Option<Vec<crate::types::BedrockValue>>,
}
impl Default for BlockSensor {
    fn default() -> Self {
        Self {
            on_break: None,
            sensor_radius: Some(16i32),
            sources: None,
        }
    }
}
