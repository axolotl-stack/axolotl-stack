use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:mob_effect`. A component that applies a mob effect to entities that get within range.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct MobEffect {
    ///If the effect is considered an ambient effect (like the ones applied by Beacons or Conduits).
    pub ambient: Option<bool>,
    ///Time in seconds to wait between each application of the effect.
    pub cooldown_time: Option<i32>,
    ///How close a hostile entity must be to have the mob effect applied.
    pub effect_range: Option<f32>,
    ///How long the applied mob effect lasts in seconds.
    pub effect_time: Option<crate::types::MolangOr<i32>>,
    ///Filter to use for conditions.
    pub entity_filter: Option<crate::types::BedrockValue>,
    ///The mob effect that is applied to entities that enter this entities effect range.
    pub mob_effect: Option<String>,
}
impl Default for MobEffect {
    fn default() -> Self {
        Self {
            ambient: Some(false),
            cooldown_time: Some(0i32),
            effect_range: Some(0.2f32),
            effect_time: Some(crate::types::MolangOr::Value(10i32)),
            entity_filter: None,
            mob_effect: None,
        }
    }
}
