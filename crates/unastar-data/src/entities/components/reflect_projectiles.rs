use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:reflect_projectiles`
#[derive(Component, Debug, Clone, Default, PartialEq)]
#[component(storage = "SparseSet")]
pub struct ReflectProjectiles {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
