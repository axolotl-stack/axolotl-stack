//! Generated definition for entity.
#[allow(unused_imports)]
use super::super::components::*;
use bevy_ecs::prelude::{Bundle, Commands, Entity};
/// Entity definition for `minecraft:chest_minecart`
pub struct ChestMinecart;
impl ChestMinecart {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:chest_minecart";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = false;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:chest_minecart`
#[derive(Bundle, Clone)]
pub struct ChestMinecartBundle {
    pub collision_box: super::super::components::CollisionBox,
    pub conditional_bandwidth_optimization:
        super::super::components::ConditionalBandwidthOptimization,
    pub inventory: super::super::components::Inventory,
    pub is_stackable: super::super::components::IsStackable,
    pub physics: super::super::components::Physics,
    pub pushable: super::super::components::Pushable,
    pub rail_movement: super::super::components::RailMovement,
    pub type_family: super::super::components::TypeFamily,
}
/// Spawn a new `minecraft:chest_minecart` entity with default Bedrock components
pub fn spawn_chest_minecart(commands: &mut Commands) -> Entity {
    commands
        .spawn(ChestMinecartBundle {
            collision_box: super::super::components::CollisionBox {
                height: Some(0.7f32),
                width: Some(0.98f32),
            },
            conditional_bandwidth_optimization:
                super::super::components::ConditionalBandwidthOptimization {
                    conditional_values: Some(vec![
                        ConditionalBandwidthOptimizationConditionalValues {
                            conditional_values: Some(vec![crate::types::BedrockValue::Object(
                                std::collections::HashMap::from([
                                    (
                                        "operator".to_string(),
                                        crate::types::BedrockValue::String("==".to_string()),
                                    ),
                                    (
                                        "subject".to_string(),
                                        crate::types::BedrockValue::String("self".to_string()),
                                    ),
                                    (
                                        "test".to_string(),
                                        crate::types::BedrockValue::String("is_moving".to_string()),
                                    ),
                                    ("value".to_string(), crate::types::BedrockValue::Bool(true)),
                                ]),
                            )]),
                            max_dropped_ticks: Some(0i32),
                            max_optimized_distance: Some(0f32),
                            use_motion_prediction_hints: None,
                        },
                    ]),
                    default_values: Some(ConditionalBandwidthOptimizationDefaultValues {
                        max_dropped_ticks: Some(20i32),
                        max_optimized_distance: Some(60f32),
                        use_motion_prediction_hints: Some(true),
                    }),
                },
            inventory: super::super::components::Inventory {
                additional_slots_per_strength: Some(0i32),
                can_be_siphoned_from: Some(true),
                container_type: Some("minecart_chest".to_string()),
                inventory_size: Some(27i32),
                private: Some(false),
                restrict_to_owner: Some(false),
            },
            is_stackable: super::super::components::IsStackable { value: true },
            physics: super::super::components::Physics {
                has_collision: Some(true),
                has_gravity: Some(true),
                push_towards_closest_space: Some(false),
            },
            pushable: super::super::components::Pushable {
                is_pushable: Some(true),
                is_pushable_by_piston: Some(true),
            },
            rail_movement: super::super::components::RailMovement {
                max_speed: Some(0.4f32),
            },
            type_family: super::super::components::TypeFamily {
                family: vec!["minecart".to_string(), "inanimate".to_string()],
            },
        })
        .id()
}
