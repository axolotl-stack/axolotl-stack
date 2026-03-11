use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:mob_effect_immunity`
#[derive(Component, Debug, Clone, Default, PartialEq)]
#[component(storage = "SparseSet")]
pub struct MobEffectImmunity {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
