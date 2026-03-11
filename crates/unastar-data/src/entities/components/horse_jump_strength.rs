use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:horse.jump_strength`. Allows this mob to jump higher when being ridden by a player.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct HorseJumpStrength {
    ///The multiplier to apply to the jumping height.
    pub value: crate::types::RangeOrVal<f32>,
}
impl Default for HorseJumpStrength {
    fn default() -> Self {
        Self {
            value: crate::types::RangeOrVal::Fixed(0f32),
        }
    }
}
