//! Generated definition for entity.
#[allow(unused_imports)]
use super::super::components::*;
use bevy_ecs::prelude::{Bundle, Commands, Entity};
/// Entity definition for `minecraft:hopper_minecart`
pub struct HopperMinecart;
impl HopperMinecart {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:hopper_minecart";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = false;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:hopper_minecart`
#[derive(Bundle, Clone)]
pub struct HopperMinecartBundle {
    pub attack: super::super::components::Attack,
    pub breathable: super::super::components::Breathable,
    pub collision_box: super::super::components::CollisionBox,
    pub conditional_bandwidth_optimization:
        super::super::components::ConditionalBandwidthOptimization,
    pub flying_speed: super::super::components::FlyingSpeed,
    pub follow_range: super::super::components::FollowRange,
    pub health: super::super::components::Health,
    pub inventory: super::super::components::Inventory,
    pub is_stackable: super::super::components::IsStackable,
    pub movement: super::super::components::Movement,
    pub physics: super::super::components::Physics,
    pub pushable: super::super::components::Pushable,
    pub rail_movement: super::super::components::RailMovement,
    pub scale: super::super::components::Scale,
    pub type_family: super::super::components::TypeFamily,
}
/// Spawn a new `minecraft:hopper_minecart` entity with default Bedrock components
pub fn spawn_hopper_minecart(commands: &mut Commands) -> Entity {
    commands
        .spawn(HopperMinecartBundle {
            attack: super::super::components::Attack {
                damage: crate::types::RangeOrVal::Fixed(2f32),
                effect_duration: Some(crate::types::MolangOr::Value(0i32)),
                effect_name: None,
            },
            breathable: super::super::components::Breathable {
                breathe_blocks: None,
                breathes_air: Some(true),
                breathes_lava: Some(false),
                breathes_solids: Some(false),
                breathes_water: Some(false),
                can_dehydrate: Some(false),
                generates_bubbles: Some(true),
                inhale_time: Some(0f32),
                non_breathe_blocks: None,
                suffocate_time: Some(-1i32),
                total_supply: Some(15i32),
            },
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
            flying_speed: super::super::components::FlyingSpeed { value: 0.02f32 },
            follow_range: super::super::components::FollowRange {
                max: None,
                min: None,
                value: crate::types::RangeOrVal::Fixed(16f32),
            },
            health: super::super::components::Health {
                max: None,
                min: None,
                value: crate::types::RangeOrVal::Fixed(20f32),
            },
            inventory: super::super::components::Inventory {
                additional_slots_per_strength: Some(0i32),
                can_be_siphoned_from: Some(true),
                container_type: Some("minecart_hopper".to_string()),
                inventory_size: Some(5i32),
                private: Some(false),
                restrict_to_owner: Some(false),
            },
            is_stackable: super::super::components::IsStackable { value: false },
            movement: super::super::components::Movement {
                max: None,
                min: None,
                value: crate::types::RangeOrVal::Fixed(0.25f32),
            },
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
            scale: super::super::components::Scale { value: 1f32 },
            type_family: super::super::components::TypeFamily {
                family: vec!["minecart".to_string(), "inanimate".to_string()],
            },
        })
        .id()
}
/// Component groups for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HopperMinecartComponentGroup {
    HopperActive,
    HopperInactive,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HopperMinecartEvent {
    EntitySpawned,
    HopperActivate,
    HopperDeactivate,
}
