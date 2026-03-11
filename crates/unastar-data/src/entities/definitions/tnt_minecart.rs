//! Generated definition for entity.
#[allow(unused_imports)]
use super::super::components::*;
use bevy_ecs::prelude::{Bundle, Commands, Entity};
/// Entity definition for `minecraft:tnt_minecart`
pub struct TntMinecart;
impl TntMinecart {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:tnt_minecart";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = false;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:tnt_minecart`
#[derive(Bundle, Clone)]
pub struct TntMinecartBundle {
    pub collision_box: super::super::components::CollisionBox,
    pub conditional_bandwidth_optimization:
        super::super::components::ConditionalBandwidthOptimization,
    pub is_stackable: super::super::components::IsStackable,
    pub physics: super::super::components::Physics,
    pub pushable: super::super::components::Pushable,
    pub rail_movement: super::super::components::RailMovement,
    pub type_family: super::super::components::TypeFamily,
}
/// Spawn a new `minecraft:tnt_minecart` entity with default Bedrock components
pub fn spawn_tnt_minecart(commands: &mut Commands) -> Entity {
    commands
        .spawn(TntMinecartBundle {
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
            is_stackable: super::super::components::IsStackable { value: false },
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
/// Component groups for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TntMinecartComponentGroup {
    Inactive,
    InstantExplodeTnt,
    PrimedTnt,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TntMinecartEvent {
    EntitySpawned,
    OnInstantPrime,
    OnPrime,
}
