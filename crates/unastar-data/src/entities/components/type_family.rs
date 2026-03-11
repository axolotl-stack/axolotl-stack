use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:type_family`. Defines the families this entity belongs to.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct TypeFamily {
    ///List of family names.
    pub family: Vec<String>,
}
impl Default for TypeFamily {
    fn default() -> Self {
        Self { family: Vec::new() }
    }
}
