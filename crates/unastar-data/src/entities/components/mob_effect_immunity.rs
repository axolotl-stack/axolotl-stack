use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:mob_effect_immunity`. Entities with this component will have an immunity to the provided mob effects.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct MobEffectImmunity {
    ///List of names of effects the entity is immune to.
    pub mob_effects: Option<Vec<String>>,
}
impl Default for MobEffectImmunity {
    fn default() -> Self {
        Self { mob_effects: None }
    }
}
