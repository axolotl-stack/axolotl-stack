use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct HurtOnConditionDamageConditions {
    ///Damage cause.
    pub cause: Option<String>,
    ///Amount of damage done each tick that the conditions are met.
    pub damage_per_tick: Option<i32>,
    ///filters
    pub filters: Option<crate::types::BedrockValue>,
}
impl Default for HurtOnConditionDamageConditions {
    fn default() -> Self {
        Self {
            cause: None,
            damage_per_tick: None,
            filters: None,
        }
    }
}
/// Bedrock component `minecraft:hurt_on_condition`. Defines a set of conditions under which an entity should take damage.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct HurtOnCondition {
    ///An array of conditions used to compare the event to.
    pub damage_conditions: Option<Vec<HurtOnConditionDamageConditions>>,
}
impl Default for HurtOnCondition {
    fn default() -> Self {
        Self {
            damage_conditions: None,
        }
    }
}
