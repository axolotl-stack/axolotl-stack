use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:mark_variant`. Additional variant value. Can be used to further differentiate variants.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct MarkVariant {
    ///The ID of the variant. By convention, 0 is the ID of the base entity
    pub value: i32,
}
impl Default for MarkVariant {
    fn default() -> Self {
        Self { value: 0i32 }
    }
}
