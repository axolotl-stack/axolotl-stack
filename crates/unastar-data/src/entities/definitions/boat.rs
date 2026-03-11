//! Generated definition for entity.
#[allow(unused_imports)]
use super::super::components::*;
use bevy_ecs::prelude::{Bundle, Commands, Entity};
/// Entity definition for `minecraft:boat`
pub struct Boat;
impl Boat {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:boat";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = false;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:boat`
#[derive(Bundle, Clone)]
pub struct BoatBundle {
    pub balloonable: super::super::components::Balloonable,
    pub buoyant: super::super::components::Buoyant,
    pub collision_box: super::super::components::CollisionBox,
    pub conditional_bandwidth_optimization:
        super::super::components::ConditionalBandwidthOptimization,
    pub hurt_on_condition: super::super::components::HurtOnCondition,
    pub inside_block_notifier: super::super::components::InsideBlockNotifier,
    pub is_collidable: super::super::components::IsCollidable,
    pub is_stackable: super::super::components::IsStackable,
    pub leashable: super::super::components::Leashable,
    pub leashable_to: super::super::components::LeashableTo,
    pub loot: super::super::components::Loot,
    pub physics: super::super::components::Physics,
    pub pushable: super::super::components::Pushable,
    pub rideable: super::super::components::Rideable,
    pub type_family: super::super::components::TypeFamily,
}
/// Spawn a new `minecraft:boat` entity with default Bedrock components
pub fn spawn_boat(commands: &mut Commands) -> Entity {
    commands
        .spawn(BoatBundle {
            balloonable: super::super::components::Balloonable {
                mass: None,
                max_distance: None,
                on_balloon: None,
                on_unballoon: None,
                soft_distance: None,
            },
            buoyant: super::super::components::Buoyant {
                apply_gravity: Some(true),
                base_buoyancy: Some(1f32),
                big_wave_probability: Some(0.03f32),
                big_wave_speed: Some(10f32),
                buoyancy: None,
                drag_down_on_buoyancy_removed: Some(0f32),
                liquid_blocks: Some(
                    vec![
                        crate ::types::BedrockValue::String("minecraft:water"
                        .to_string()), crate
                        ::types::BedrockValue::String("minecraft:flowing_water"
                        .to_string())
                    ],
                ),
                simulate_waves: Some(true),
            },
            collision_box: super::super::components::CollisionBox {
                height: Some(0.455f32),
                width: Some(1.4f32),
            },
            conditional_bandwidth_optimization: super::super::components::ConditionalBandwidthOptimization {
                conditional_values: Some(
                    vec![
                        ConditionalBandwidthOptimizationConditionalValues {
                        conditional_values : Some(vec![crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("subject"
                        .to_string(), crate ::types::BedrockValue::String("self"
                        .to_string())), ("test".to_string(), crate
                        ::types::BedrockValue::String("is_moving".to_string()))]))]),
                        max_dropped_ticks : Some(0i32), max_optimized_distance :
                        Some(0f32), use_motion_prediction_hints : Some(true) }
                    ],
                ),
                default_values: Some(ConditionalBandwidthOptimizationDefaultValues {
                    max_dropped_ticks: Some(20i32),
                    max_optimized_distance: Some(60f32),
                    use_motion_prediction_hints: Some(true),
                }),
            },
            hurt_on_condition: super::super::components::HurtOnCondition {
                damage_conditions: None,
            },
            inside_block_notifier: super::super::components::InsideBlockNotifier {
                block_list: None,
            },
            is_collidable: super::super::components::IsCollidable,
            is_stackable: super::super::components::IsStackable {
                value: false,
            },
            leashable: super::super::components::Leashable {
                can_be_cut: Some(true),
                can_be_stolen: Some(false),
                hard_distance: Some(6f32),
                max_distance: Some(0f32),
                on_leash: None,
                on_unleash: None,
                on_unleash_interact_only: Some(false),
                presets: Some(
                    vec![
                        LeashablePresets { filter : Some(crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("subject"
                        .to_string(), crate ::types::BedrockValue::String("other"
                        .to_string())), ("test".to_string(), crate
                        ::types::BedrockValue::String("is_family".to_string())), ("value"
                        .to_string(), crate ::types::BedrockValue::String("happy_ghast"
                        .to_string()))]))), hard_distance : None, max_distance : None,
                        rotation_adjustment : Some(90f32), soft_distance : None,
                        spring_type : Some("quad_dampened".to_string()) },
                        LeashablePresets { filter : None, hard_distance : Some(4f32),
                        max_distance : None, rotation_adjustment : Some(90f32),
                        soft_distance : Some(2f32), spring_type : None }
                    ],
                ),
                soft_distance: Some(4f32),
            },
            leashable_to: super::super::components::LeashableTo {
                can_retrieve_from: Some(false),
            },
            loot: super::super::components::Loot {
                table: "loot_tables/entities/boat.json".to_string(),
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
                dismount_mode: Some("default".to_string()),
                family_types: None,
                interact_text: Some("action.interact.ride.boat".to_string()),
                on_rider_enter_event: None,
                on_rider_exit_event: None,
                passenger_max_width: Some(1.375f32),
                pull_in_entities: Some(true),
                rider_can_interact: Some(false),
                seat_count: Some(2i32),
                seats: Some(
                    vec![
                        RideableSeats { camera_relax_distance_smoothing : None,
                        lock_rider_rotation : Some(90f32), max_rider_count : Some(1i32),
                        min_rider_count : Some(0i32), position : None, rotate_rider_by :
                        Some(crate ::types::MolangOr::Value(- 90f32)),
                        third_person_camera_radius : None }, RideableSeats {
                        camera_relax_distance_smoothing : None, lock_rider_rotation :
                        Some(90f32), max_rider_count : Some(2i32), min_rider_count :
                        Some(2i32), position : None, rotate_rider_by : Some(crate
                        ::types::MolangOr::Expr("query.has_any_family('blaze', 'creeper', 'enderman', 'illager', 'magmacube', 'piglin', 'player', 'skeleton', 'slime', 'villager', 'wandering_trader', 'witch', 'zombie', 'zombie_pigman', 'happy_ghast', 'copper_golem') ? -90 : 0"
                        .to_string())), third_person_camera_radius : None },
                        RideableSeats { camera_relax_distance_smoothing : None,
                        lock_rider_rotation : Some(90f32), max_rider_count : Some(2i32),
                        min_rider_count : Some(2i32), position : None, rotate_rider_by :
                        Some(crate
                        ::types::MolangOr::Expr("query.has_any_family('blaze', 'creeper', 'enderman', 'illager', 'magmacube', 'piglin', 'player', 'skeleton', 'slime', 'villager', 'wandering_trader', 'witch', 'zombie', 'zombie_pigman', 'happy_ghast', 'copper_golem') ? -90 : 0"
                        .to_string())), third_person_camera_radius : None }
                    ],
                ),
            },
            type_family: super::super::components::TypeFamily {
                family: vec!["boat".to_string(), "inanimate".to_string()],
            },
        })
        .id()
}
/// Component groups for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BoatComponentGroup {
    AboveBubbleColumnDown,
    AboveBubbleColumnUp,
    CanRideBamboo,
    CanRideDefault,
    Floating,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BoatEvent {
    AddCanRide,
    EnteredBubbleColumnDown,
    EnteredBubbleColumnUp,
    EntitySpawned,
    ExitedBubbleColumn,
    Sink,
}
