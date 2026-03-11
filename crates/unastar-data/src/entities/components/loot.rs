use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:loot`. sets the loot table for what items this entity drops upon death.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct Loot {
    ///The path to the loot table, relative to the Behavior Pack's root.
    pub table: String,
}
impl Default for Loot {
    fn default() -> Self {
        Self {
            table: "".to_string(),
        }
    }
}
