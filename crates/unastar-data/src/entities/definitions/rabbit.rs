//! Generated definition for entity.
#[allow(unused_imports)]
use super::super::components::*;
use bevy_ecs::prelude::{Bundle, Commands, Entity};
/// Entity definition for `minecraft:rabbit`
pub struct Rabbit;
impl Rabbit {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:rabbit";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:rabbit`
#[derive(Bundle, Clone)]
pub struct RabbitBundle {
    pub balloonable: super::super::components::Balloonable,
    pub behavior_avoid_mob_type: super::super::components::BehaviorAvoidMobType,
    pub behavior_breed: super::super::components::BehaviorBreed,
    pub behavior_float: super::super::components::BehaviorFloat,
    pub behavior_look_at_player: super::super::components::BehaviorLookAtPlayer,
    pub behavior_panic: super::super::components::BehaviorPanic,
    pub behavior_raid_garden: super::super::components::BehaviorRaidGarden,
    pub behavior_random_stroll: super::super::components::BehaviorRandomStroll,
    pub behavior_tempt: super::super::components::BehaviorTempt,
    pub block_climber: super::super::components::BlockClimber,
    pub breathable: super::super::components::Breathable,
    pub can_climb: super::super::components::CanClimb,
    pub collision_box: super::super::components::CollisionBox,
    pub conditional_bandwidth_optimization:
        super::super::components::ConditionalBandwidthOptimization,
    pub despawn: super::super::components::Despawn,
    pub health: super::super::components::Health,
    pub hurt_on_condition: super::super::components::HurtOnCondition,
    pub is_hidden_when_invisible: super::super::components::IsHiddenWhenInvisible,
    pub jump_dynamic: super::super::components::JumpDynamic,
    pub leashable: super::super::components::Leashable,
    pub movement: super::super::components::Movement,
    pub movement_skip: super::super::components::MovementSkip,
    pub nameable: super::super::components::Nameable,
    pub navigation_walk: super::super::components::NavigationWalk,
    pub physics: super::super::components::Physics,
    pub pushable: super::super::components::Pushable,
    pub scale: super::super::components::Scale,
    pub type_family: super::super::components::TypeFamily,
}
/// Spawn a new `minecraft:rabbit` entity with default Bedrock components
pub fn spawn_rabbit(commands: &mut Commands) -> Entity {
    commands
        .spawn(RabbitBundle {
            balloonable: super::super::components::Balloonable {
                mass: Some(0.4f32),
                max_distance: None,
                on_balloon: None,
                on_unballoon: None,
                soft_distance: None,
            },
            behavior_avoid_mob_type: super::super::components::BehaviorAvoidMobType {
                avoid_mob_sound: Some("undefined".to_string()),
                avoid_target_xz: Some(16i32),
                avoid_target_y: Some(7i32),
                control_flags: Some(BehaviorAvoidMobTypeControlFlags {
                }),
                entity_types: Some(
                    vec![
                        BehaviorAvoidMobTypeEntityTypes { check_if_outnumbered : None,
                        cooldown : None, filters : Some(crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("subject"
                        .to_string(), crate ::types::BedrockValue::String("other"
                        .to_string())), ("test".to_string(), crate
                        ::types::BedrockValue::String("is_family".to_string())), ("value"
                        .to_string(), crate ::types::BedrockValue::String("player"
                        .to_string()))]))), max_dist : Some(8f32), max_flee : None,
                        max_height : None, must_see : None, must_see_forget_duration :
                        None, priority : None, reevaluate_description : None,
                        sprint_speed_multiplier : Some(1.8f32), walk_speed_multiplier :
                        Some(1.5f32), within_default : None },
                        BehaviorAvoidMobTypeEntityTypes { check_if_outnumbered : None,
                        cooldown : None, filters : Some(crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("subject"
                        .to_string(), crate ::types::BedrockValue::String("other"
                        .to_string())), ("test".to_string(), crate
                        ::types::BedrockValue::String("is_family".to_string())), ("value"
                        .to_string(), crate ::types::BedrockValue::String("wolf"
                        .to_string()))]))), max_dist : Some(4f32), max_flee : None,
                        max_height : None, must_see : None, must_see_forget_duration :
                        None, priority : None, reevaluate_description : None,
                        sprint_speed_multiplier : Some(1.8f32), walk_speed_multiplier :
                        Some(1.5f32), within_default : None },
                        BehaviorAvoidMobTypeEntityTypes { check_if_outnumbered : None,
                        cooldown : None, filters : Some(crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("subject"
                        .to_string(), crate ::types::BedrockValue::String("other"
                        .to_string())), ("test".to_string(), crate
                        ::types::BedrockValue::String("is_family".to_string())), ("value"
                        .to_string(), crate ::types::BedrockValue::String("monster"
                        .to_string()))]))), max_dist : Some(4f32), max_flee : None,
                        max_height : None, must_see : None, must_see_forget_duration :
                        None, priority : None, reevaluate_description : None,
                        sprint_speed_multiplier : Some(1.5f32), walk_speed_multiplier :
                        Some(1.5f32), within_default : None }
                    ],
                ),
                ignore_visibility: Some(false),
                ignore_visibilty: None,
                max_dist: Some(3f32),
                max_flee: Some(10f32),
                on_escape_event: Some(
                    crate::types::BedrockValue::Object(
                        std::collections::HashMap::from([
                            (
                                "event".to_string(),
                                crate::types::BedrockValue::String("".to_string()),
                            ),
                            (
                                "filters".to_string(),
                                crate::types::BedrockValue::Object(
                                    std::collections::HashMap::from([
                                        ("AND".to_string(), crate::types::BedrockValue::Null),
                                        ("NOT".to_string(), crate::types::BedrockValue::Null),
                                        ("OR".to_string(), crate::types::BedrockValue::Null),
                                        ("all".to_string(), crate::types::BedrockValue::Null),
                                        ("all_of".to_string(), crate::types::BedrockValue::Null),
                                        ("any".to_string(), crate::types::BedrockValue::Null),
                                        ("any_of".to_string(), crate::types::BedrockValue::Null),
                                        ("none_of".to_string(), crate::types::BedrockValue::Null),
                                    ]),
                                ),
                            ),
                            (
                                "target".to_string(),
                                crate::types::BedrockValue::String("self".to_string()),
                            ),
                        ]),
                    ),
                ),
                priority: Some(BehaviorAvoidMobTypePriority {}),
                probability_per_strength: Some(1f32),
                remove_target: Some(false),
                sound_interval: Some(crate::types::RangeOrVal::Range {
                    min: 3f32,
                    max: 8f32,
                }),
                sprint_distance: Some(7f32),
                sprint_speed_multiplier: Some(1f32),
                walk_speed_multiplier: Some(1f32),
            },
            behavior_breed: super::super::components::BehaviorBreed {
                priority: Some(BehaviorBreedPriority {}),
                speed_multiplier: Some(BehaviorBreedSpeedMultiplier {}),
            },
            behavior_float: super::super::components::BehaviorFloat {
                chance_per_tick_to_float: Some(0.8f32),
                priority: Some(BehaviorFloatPriority {}),
                sink_with_passengers: Some(false),
                time_under_water_to_dismount_passengers: Some(0f32),
            },
            behavior_look_at_player: super::super::components::BehaviorLookAtPlayer {
                angle_of_view_horizontal: Some(360i32),
                angle_of_view_vertical: Some(360i32),
                look_distance: Some(8f32),
                look_time: None,
                priority: Some(BehaviorLookAtPlayerPriority {}),
                probability: Some(0.02f32),
                target_distance: None,
            },
            behavior_panic: super::super::components::BehaviorPanic {
                damage_sources: Some(
                    vec![
                        "[campfire, fire, fire_tick, freezing, lava, lightning, magma, soul_campfire, temperature, entity_attack, entity_explosion, fireworks, magic, projectile, ram_attack, sonic_boom, wither, mace_smash]"
                        .to_string()
                    ],
                ),
                force: Some(false),
                ignore_mob_damage: Some(false),
                panic_sound: None,
                prefer_water: Some(false),
                priority: Some(BehaviorPanicPriority {}),
                sound_interval: None,
                speed_multiplier: Some(BehaviorPanicSpeedMultiplier {}),
            },
            behavior_raid_garden: super::super::components::BehaviorRaidGarden {
                blocks: Some(
                    vec![
                        crate ::types::BedrockValue::String("minecraft:carrots"
                        .to_string())
                    ],
                ),
                eat_delay: Some(2i32),
                full_delay: Some(100i32),
                goal_radius: Some(1f32),
                initial_eat_delay: Some(0i32),
                max_to_eat: Some(6i32),
                priority: Some(BehaviorRaidGardenPriority {}),
                search_height: None,
                search_range: Some(16i32),
                speed_multiplier: Some(BehaviorRaidGardenSpeedMultiplier {
                }),
            },
            behavior_random_stroll: super::super::components::BehaviorRandomStroll {
                interval: Some(120i32),
                priority: Some(BehaviorRandomStrollPriority {}),
                speed_multiplier: Some(BehaviorRandomStrollSpeedMultiplier {
                }),
                xz_dist: Some(2i32),
                y_dist: Some(1i32),
            },
            behavior_tempt: super::super::components::BehaviorTempt {
                can_get_scared: Some(false),
                can_tempt_vertically: Some(false),
                can_tempt_while_ridden: Some(false),
                items: Some(
                    vec![
                        crate ::types::BedrockValue::String("golden_carrot".to_string()),
                        crate ::types::BedrockValue::String("carrot".to_string()), crate
                        ::types::BedrockValue::String("dandelion".to_string())
                    ],
                ),
                on_end: None,
                on_start: None,
                priority: Some(BehaviorTemptPriority {}),
                sound_interval: None,
                speed_multiplier: Some(BehaviorTemptSpeedMultiplier {}),
                stop_distance: Some(1.5f32),
                tempt_sound: None,
                within_radius: Some(0f32),
            },
            block_climber: super::super::components::BlockClimber,
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
                suffocate_time: Some(0i32),
                total_supply: Some(15i32),
            },
            can_climb: super::super::components::CanClimb,
            collision_box: super::super::components::CollisionBox {
                height: Some(0.67f32),
                width: Some(0.67f32),
            },
            conditional_bandwidth_optimization: super::super::components::ConditionalBandwidthOptimization {
                conditional_values: None,
                default_values: None,
            },
            despawn: super::super::components::Despawn {
                despawn_from_chance: Some(true),
                despawn_from_distance: Some(DespawnDespawnFromDistance {
                    max_distance: None,
                    min_distance: None,
                }),
                despawn_from_inactivity: Some(true),
                despawn_from_simulation_edge: Some(true),
                filters: None,
                min_range_inactivity_timer: Some(30i32),
                min_range_random_chance: Some(800i32),
                remove_child_entities: Some(false),
            },
            health: super::super::components::Health {
                max: Some(3f32),
                min: None,
                value: crate::types::RangeOrVal::Fixed(3f32),
            },
            hurt_on_condition: super::super::components::HurtOnCondition {
                damage_conditions: None,
            },
            is_hidden_when_invisible: super::super::components::IsHiddenWhenInvisible,
            jump_dynamic: super::super::components::JumpDynamic {
                fast_skip_data: None,
                regular_skip_data: None,
            },
            leashable: super::super::components::Leashable {
                can_be_cut: Some(true),
                can_be_stolen: Some(false),
                hard_distance: Some(6f32),
                max_distance: Some(0f32),
                on_leash: None,
                on_unleash: None,
                on_unleash_interact_only: Some(false),
                presets: None,
                soft_distance: Some(4f32),
            },
            movement: super::super::components::Movement {
                max: None,
                min: None,
                value: crate::types::RangeOrVal::Fixed(0.3f32),
            },
            movement_skip: super::super::components::MovementSkip {
                max_turn: Some(30f32),
            },
            nameable: super::super::components::Nameable {
                allow_name_tag_renaming: Some(true),
                always_show: Some(false),
                default_trigger: None,
                name_actions: None,
            },
            navigation_walk: super::super::components::NavigationWalk {
                avoid_damage_blocks: Some(false),
                avoid_portals: Some(false),
                avoid_sun: Some(false),
                avoid_water: Some(true),
                blocks_to_avoid: None,
                can_breach: Some(false),
                can_break_doors: Some(false),
                can_float: None,
                can_jump: Some(true),
                can_open_doors: Some(false),
                can_open_iron_doors: Some(false),
                can_pass_doors: Some(true),
                can_path_from_air: Some(false),
                can_path_over_lava: Some(false),
                can_path_over_water: Some(true),
                can_sink: Some(true),
                can_swim: Some(false),
                can_walk: Some(true),
                can_walk_in_lava: Some(false),
                is_amphibious: Some(false),
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
            scale: super::super::components::Scale {
                value: 0.6f32,
            },
            type_family: super::super::components::TypeFamily {
                family: vec![
                    "rabbit".to_string(), "lightweight".to_string(), "mob".to_string()
                ],
            },
        })
        .id()
}
/// Component groups for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RabbitComponentGroup {
    Adult,
    Baby,
    CoatBlack,
    CoatBrown,
    CoatDesert,
    CoatSalt,
    CoatSplotched,
    CoatWhite,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RabbitEvent {
    GrowUp,
    InDesert,
    InSnow,
    EntityBorn,
    EntitySpawned,
}
