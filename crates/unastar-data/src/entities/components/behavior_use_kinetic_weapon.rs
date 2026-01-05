use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:behavior.use_kinetic_weapon`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct BehaviorUseKineticWeapon {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
