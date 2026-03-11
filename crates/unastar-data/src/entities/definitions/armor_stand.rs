//! Generated definition for entity.
#[allow(unused_imports)]
use super::super::components::*;
use bevy_ecs::prelude::{Bundle, Commands, Entity};
/// Entity definition for `minecraft:armor_stand`
pub struct ArmorStand;
impl ArmorStand {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:armor_stand";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = false;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:armor_stand`
#[derive(Bundle, Clone)]
pub struct ArmorStandBundle {
    pub collision_box: super::super::components::CollisionBox,
    pub conditional_bandwidth_optimization:
        super::super::components::ConditionalBandwidthOptimization,
    pub health: super::super::components::Health,
    pub hurt_on_condition: super::super::components::HurtOnCondition,
    pub knockback_resistance: super::super::components::KnockbackResistance,
    pub loot: super::super::components::Loot,
    pub nameable: super::super::components::Nameable,
    pub persistent: super::super::components::Persistent,
    pub physics: super::super::components::Physics,
    pub pushable: super::super::components::Pushable,
    pub type_family: super::super::components::TypeFamily,
}
/// Spawn a new `minecraft:armor_stand` entity with default Bedrock components
pub fn spawn_armor_stand(commands: &mut Commands) -> Entity {
    commands
        .spawn(ArmorStandBundle {
            collision_box: super::super::components::CollisionBox {
                height: Some(1.975f32),
                width: Some(0.5f32),
            },
            conditional_bandwidth_optimization:
                super::super::components::ConditionalBandwidthOptimization {
                    conditional_values: None,
                    default_values: None,
                },
            health: super::super::components::Health {
                max: Some(6f32),
                min: None,
                value: crate::types::RangeOrVal::Fixed(6f32),
            },
            hurt_on_condition: super::super::components::HurtOnCondition {
                damage_conditions: None,
            },
            knockback_resistance: super::super::components::KnockbackResistance {
                max: None,
                min: None,
                value: crate::types::RangeOrVal::Fixed(1f32),
            },
            loot: super::super::components::Loot {
                table: "loot_tables/entities/armor_stand.json".to_string(),
            },
            nameable: super::super::components::Nameable {
                allow_name_tag_renaming: Some(true),
                always_show: Some(false),
                default_trigger: None,
                name_actions: None,
            },
            persistent: super::super::components::Persistent,
            physics: super::super::components::Physics {
                has_collision: Some(true),
                has_gravity: Some(true),
                push_towards_closest_space: Some(false),
            },
            pushable: super::super::components::Pushable {
                is_pushable: Some(false),
                is_pushable_by_piston: Some(true),
            },
            type_family: super::super::components::TypeFamily {
                family: vec![
                    "armor_stand".to_string(),
                    "inanimate".to_string(),
                    "mob".to_string(),
                ],
            },
        })
        .id()
}
