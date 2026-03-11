use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:loot`. sets the loot table for what items this entity drops upon death.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct Loot {
    /// table
    pub table: String,
}
impl Default for Loot {
    fn default() -> Self {
        Self {
            table: String::new(),
        }
    }
}
