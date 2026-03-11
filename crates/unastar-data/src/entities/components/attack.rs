use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:attack`. Defines an entity's melee attack and any additional effects on it.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct Attack {
    ///Range of the random amount of damage the melee attack deals. A negative value can heal the entity instead of hurting it.
    pub damage: crate::types::RangeOrVal<f32>,
    ///Duration in seconds of the status ailment applied to the damaged entity.
    pub effect_duration: Option<crate::types::MolangOr<i32>>,
    ///Identifier of the status ailment to apply to an entity attacked by this entity's melee attack.
    pub effect_name: Option<String>,
}
impl Default for Attack {
    fn default() -> Self {
        Self {
            damage: crate::types::RangeOrVal::Fixed(2f32),
            effect_duration: Some(crate::types::MolangOr::Value(0i32)),
            effect_name: None,
        }
    }
}
