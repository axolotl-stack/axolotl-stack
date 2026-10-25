//! Generated definition for entity.
#[allow(unused_imports)]
use super::super::components::*;
use bevy_ecs::prelude::{Bundle, Commands, Entity};
/// Entity definition for `minecraft:cushion`
pub struct Cushion;
impl Cushion {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:cushion";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = false;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:cushion`
#[derive(Bundle, Clone)]
pub struct CushionBundle {
    pub attack: super::super::components::Attack,
    pub breathable: super::super::components::Breathable,
    pub collision_box: super::super::components::CollisionBox,
    pub conditional_bandwidth_optimization:
        super::super::components::ConditionalBandwidthOptimization,
    pub flying_speed: super::super::components::FlyingSpeed,
    pub follow_range: super::super::components::FollowRange,
    pub health: super::super::components::Health,
    pub hurt_on_condition: super::super::components::HurtOnCondition,
    pub inventory: super::super::components::Inventory,
    pub movement: super::super::components::Movement,
    pub nameable: super::super::components::Nameable,
    pub physics: super::super::components::Physics,
    pub pushable: super::super::components::Pushable,
    pub rideable: super::super::components::Rideable,
    pub scale: super::super::components::Scale,
    pub type_family: super::super::components::TypeFamily,
    pub variant: super::super::components::Variant,
}
/// Spawn a new `minecraft:cushion` entity with default Bedrock components
pub fn spawn_cushion(commands: &mut Commands) -> Entity {
    commands
        .spawn(CushionBundle {
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
                height: Some(0.249f32),
                width: Some(0.999f32),
            },
            conditional_bandwidth_optimization:
                super::super::components::ConditionalBandwidthOptimization {
                    conditional_values: None,
                    default_values: None,
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
            hurt_on_condition: super::super::components::HurtOnCondition {
                damage_conditions: None,
            },
            inventory: super::super::components::Inventory {
                additional_slots_per_strength: Some(0i32),
                can_be_siphoned_from: Some(false),
                container_type: Some("none".to_string()),
                inventory_size: Some(5i32),
                private: Some(false),
                restrict_to_owner: Some(false),
            },
            movement: super::super::components::Movement {
                max: None,
                min: None,
                value: crate::types::RangeOrVal::Fixed(0.25f32),
            },
            nameable: super::super::components::Nameable {
                allow_name_tag_renaming: Some(false),
                always_show: Some(false),
                default_trigger: None,
                name_actions: None,
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
            rideable: super::super::components::Rideable {
                controlling_seat: Some(0i32),
                crouching_skip_interact: Some(true),
                dismount_mode: Some("on_top_center".to_string()),
                family_types: None,
                interact_text: Some("action.interact.ride.cushion".to_string()),
                on_rider_enter_event: None,
                on_rider_exit_event: None,
                passenger_max_width: Some(0f32),
                pull_in_entities: Some(false),
                rider_can_interact: Some(false),
                seat_count: Some(1i32),
                seats: Some(vec![RideableSeats {
                    camera_relax_distance_smoothing: None,
                    lock_rider_rotation: None,
                    max_rider_count: None,
                    min_rider_count: None,
                    position: None,
                    rotate_rider_by: Some(crate::types::MolangOr::Value(-90f32)),
                    third_person_camera_radius: None,
                }]),
            },
            scale: super::super::components::Scale { value: 1f32 },
            type_family: super::super::components::TypeFamily {
                family: vec![
                    "cushion".to_string(),
                    "inanimate".to_string(),
                    "actor".to_string(),
                ],
            },
            variant: super::super::components::Variant { value: 15i32 },
        })
        .id()
}
