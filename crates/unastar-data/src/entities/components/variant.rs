use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:variant`. Used to differentiate the component group of a variant of an entity from others (e.g. ocelot, villager) Parameters
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct Variant {
    ///The ID of the variant. By convention, 0 is the ID of the base entity
    pub value: i32,
}
impl Default for Variant {
    fn default() -> Self {
        Self { value: 0i32 }
    }
}
