use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:knockback_resistance`. Specifies the initial value of a specific attribute for an entity when spawned.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct KnockbackResistance {
    ///The maximum starting health an entity has.
    pub max: Option<f32>,
    ///The minimum starting health an entity has.
    pub min: Option<f32>,
    ///The amount of health an entity to start with by default.
    pub value: crate::types::RangeOrVal<f32>,
}
impl Default for KnockbackResistance {
    fn default() -> Self {
        Self {
            max: None,
            min: None,
            value: crate::types::RangeOrVal::Fixed(0f32),
        }
    }
}
