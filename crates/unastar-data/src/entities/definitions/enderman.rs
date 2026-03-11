//! Generated definition for entity.
#[allow(unused_imports)]
use super::super::components::*;
use bevy_ecs::prelude::{Bundle, Commands, Entity};
/// Entity definition for `minecraft:enderman`
pub struct Enderman;
impl Enderman {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:enderman";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:enderman`
#[derive(Bundle, Clone)]
pub struct EndermanBundle {
    pub attack: super::super::components::Attack,
    pub behavior_float: super::super::components::BehaviorFloat,
    pub behavior_hurt_by_target: super::super::components::BehaviorHurtByTarget,
    pub behavior_look_at_player: super::super::components::BehaviorLookAtPlayer,
    pub behavior_nearest_attackable_target:
        super::super::components::BehaviorNearestAttackableTarget,
    pub behavior_place_block: super::super::components::BehaviorPlaceBlock,
    pub behavior_random_look_around: super::super::components::BehaviorRandomLookAround,
    pub behavior_random_stroll: super::super::components::BehaviorRandomStroll,
    pub behavior_take_block: super::super::components::BehaviorTakeBlock,
    pub breathable: super::super::components::Breathable,
    pub can_climb: super::super::components::CanClimb,
    pub collision_box: super::super::components::CollisionBox,
    pub conditional_bandwidth_optimization:
        super::super::components::ConditionalBandwidthOptimization,
    pub despawn: super::super::components::Despawn,
    pub experience_reward: super::super::components::ExperienceReward,
    pub follow_range: super::super::components::FollowRange,
    pub health: super::super::components::Health,
    pub hurt_on_condition: super::super::components::HurtOnCondition,
    pub is_hidden_when_invisible: super::super::components::IsHiddenWhenInvisible,
    pub jump_static: super::super::components::JumpStatic,
    pub looked_at: super::super::components::LookedAt,
    pub loot: super::super::components::Loot,
    pub movement_basic: super::super::components::MovementBasic,
    pub nameable: super::super::components::Nameable,
    pub navigation_walk: super::super::components::NavigationWalk,
    pub physics: super::super::components::Physics,
    pub pushable: super::super::components::Pushable,
    pub renders_when_invisible: super::super::components::RendersWhenInvisible,
    pub teleport: super::super::components::Teleport,
    pub type_family: super::super::components::TypeFamily,
    pub variable_max_auto_step: super::super::components::VariableMaxAutoStep,
}
/// Spawn a new `minecraft:enderman` entity with default Bedrock components
pub fn spawn_enderman(commands: &mut Commands) -> Entity {
    commands
        .spawn(EndermanBundle {
            attack: super::super::components::Attack {
                damage: crate::types::RangeOrVal::Fixed(7f32),
                effect_duration: Some(crate::types::MolangOr::Value(0i32)),
                effect_name: None,
            },
            behavior_float: super::super::components::BehaviorFloat {
                chance_per_tick_to_float: Some(0.8f32),
                priority: Some(BehaviorFloatPriority {}),
                sink_with_passengers: Some(false),
                time_under_water_to_dismount_passengers: Some(0f32),
            },
            behavior_hurt_by_target: super::super::components::BehaviorHurtByTarget {
                alert_same_type: Some(false),
                entity_types: None,
                hurt_owner: Some(false),
                priority: Some(BehaviorHurtByTargetPriority {}),
            },
            behavior_look_at_player: super::super::components::BehaviorLookAtPlayer {
                angle_of_view_horizontal: Some(360i32),
                angle_of_view_vertical: Some(360i32),
                look_distance: Some(8f32),
                look_time: None,
                priority: Some(BehaviorLookAtPlayerPriority {}),
                probability: Some(8f32),
                target_distance: None,
            },
            behavior_nearest_attackable_target:
                super::super::components::BehaviorNearestAttackableTarget {
                    attack_interval: Some(crate::types::BedrockValue::Integer(10i64)),
                    attack_interval_min: None,
                    attack_owner: Some(false),
                    control_flags: Some(BehaviorNearestAttackableTargetControlFlags {}),
                    entity_types: Some(vec![BehaviorNearestAttackableTargetEntityTypes {
                        check_if_outnumbered: None,
                        cooldown: None,
                        filters: Some(crate::types::BedrockValue::Object(
                            std::collections::HashMap::from([
                                (
                                    "subject".to_string(),
                                    crate::types::BedrockValue::String("other".to_string()),
                                ),
                                (
                                    "test".to_string(),
                                    crate::types::BedrockValue::String("is_family".to_string()),
                                ),
                                (
                                    "value".to_string(),
                                    crate::types::BedrockValue::String("endermite".to_string()),
                                ),
                            ]),
                        )),
                        max_dist: Some(64f32),
                        max_flee: None,
                        max_height: None,
                        must_see: None,
                        must_see_forget_duration: None,
                        priority: None,
                        reevaluate_description: None,
                        sprint_speed_multiplier: None,
                        walk_speed_multiplier: None,
                        within_default: None,
                    }]),
                    must_reach: Some(false),
                    must_see: Some(true),
                    must_see_forget_duration: Some(3f32),
                    persist_time: Some(0f32),
                    priority: Some(BehaviorNearestAttackableTargetPriority {}),
                    reselect_targets: Some(false),
                    scan_interval: Some(10i32),
                    set_persistent: Some(false),
                    target_acquisition_probability: Some(1f32),
                    target_invisible_multiplier: Some(0.7f32),
                    target_search_height: Some(-1f32),
                    target_sneak_visibility_multiplier: Some(0.8f32),
                    within_radius: Some(0f32),
                },
            behavior_place_block: super::super::components::BehaviorPlaceBlock {
                affected_by_griefing_rule: None,
                can_place: None,
                chance: Some(0.0005f32),
                on_place: None,
                placeable_carried_blocks: None,
                priority: Some(BehaviorPlaceBlockPriority {}),
                randomly_placeable_blocks: None,
                xz_range: None,
                y_range: None,
            },
            behavior_random_look_around: super::super::components::BehaviorRandomLookAround {
                angle_of_view_horizontal: None,
                angle_of_view_vertical: None,
                look_distance: None,
                look_time: None,
                priority: Some(BehaviorRandomLookAroundPriority {}),
                probability: None,
            },
            behavior_random_stroll: super::super::components::BehaviorRandomStroll {
                interval: Some(120i32),
                priority: Some(BehaviorRandomStrollPriority {}),
                speed_multiplier: Some(BehaviorRandomStrollSpeedMultiplier {}),
                xz_dist: Some(10i32),
                y_dist: Some(7i32),
            },
            behavior_take_block: super::super::components::BehaviorTakeBlock {
                affected_by_griefing_rule: None,
                blocks: Some(vec![
                    crate::types::BedrockValue::String("dirt".to_string()),
                    crate::types::BedrockValue::String("grass_block".to_string()),
                    crate::types::BedrockValue::String("podzol".to_string()),
                    crate::types::BedrockValue::String("coarse_dirt".to_string()),
                    crate::types::BedrockValue::String("mycelium".to_string()),
                    crate::types::BedrockValue::String("dirt_with_roots".to_string()),
                    crate::types::BedrockValue::String("moss_block".to_string()),
                    crate::types::BedrockValue::String("pale_moss_block".to_string()),
                    crate::types::BedrockValue::String("muddy_mangrove_roots".to_string()),
                    crate::types::BedrockValue::String("mud".to_string()),
                    crate::types::BedrockValue::String("sand".to_string()),
                    crate::types::BedrockValue::String("red_sand".to_string()),
                    crate::types::BedrockValue::String("gravel".to_string()),
                    crate::types::BedrockValue::String("brown_mushroom".to_string()),
                    crate::types::BedrockValue::String("red_mushroom".to_string()),
                    crate::types::BedrockValue::String("tnt".to_string()),
                    crate::types::BedrockValue::String("cactus".to_string()),
                    crate::types::BedrockValue::String("cactus_flower".to_string()),
                    crate::types::BedrockValue::String("clay".to_string()),
                    crate::types::BedrockValue::String("pumpkin".to_string()),
                    crate::types::BedrockValue::String("carved_pumpkin".to_string()),
                    crate::types::BedrockValue::String("melon_block".to_string()),
                    crate::types::BedrockValue::String("crimson_fungus".to_string()),
                    crate::types::BedrockValue::String("crimson_nylium".to_string()),
                    crate::types::BedrockValue::String("crimson_roots".to_string()),
                    crate::types::BedrockValue::String("warped_fungus".to_string()),
                    crate::types::BedrockValue::String("warped_nylium".to_string()),
                    crate::types::BedrockValue::String("warped_roots".to_string()),
                    crate::types::BedrockValue::String("dandelion".to_string()),
                    crate::types::BedrockValue::String("open_eyeblossom".to_string()),
                    crate::types::BedrockValue::String("closed_eyeblossom".to_string()),
                    crate::types::BedrockValue::String("poppy".to_string()),
                    crate::types::BedrockValue::String("blue_orchid".to_string()),
                    crate::types::BedrockValue::String("allium".to_string()),
                    crate::types::BedrockValue::String("azure_bluet".to_string()),
                    crate::types::BedrockValue::String("red_tulip".to_string()),
                    crate::types::BedrockValue::String("orange_tulip".to_string()),
                    crate::types::BedrockValue::String("white_tulip".to_string()),
                    crate::types::BedrockValue::String("pink_tulip".to_string()),
                    crate::types::BedrockValue::String("oxeye_daisy".to_string()),
                    crate::types::BedrockValue::String("cornflower".to_string()),
                    crate::types::BedrockValue::String("lily_of_the_valley".to_string()),
                    crate::types::BedrockValue::String("wither_rose".to_string()),
                    crate::types::BedrockValue::String("torchflower".to_string()),
                ]),
                can_take: None,
                chance: Some(0.05f32),
                on_take: None,
                priority: Some(BehaviorTakeBlockPriority {}),
                requires_line_of_sight: None,
                xz_range: None,
                y_range: None,
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
                suffocate_time: Some(0i32),
                total_supply: Some(15i32),
            },
            can_climb: super::super::components::CanClimb,
            collision_box: super::super::components::CollisionBox {
                height: Some(2.9f32),
                width: Some(0.6f32),
            },
            conditional_bandwidth_optimization:
                super::super::components::ConditionalBandwidthOptimization {
                    conditional_values: None,
                    default_values: Some(ConditionalBandwidthOptimizationDefaultValues {
                        max_dropped_ticks: Some(10i32),
                        max_optimized_distance: Some(80f32),
                        use_motion_prediction_hints: Some(true),
                    }),
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
            experience_reward: super::super::components::ExperienceReward {
                on_bred: Some(crate::types::MolangOr::Value(0f32)),
                on_death: Some(crate::types::MolangOr::Expr(
                    "query.last_hit_by_player ? 5 : 0".to_string(),
                )),
            },
            follow_range: super::super::components::FollowRange {
                max: Some(64f32),
                min: None,
                value: crate::types::RangeOrVal::Fixed(64f32),
            },
            health: super::super::components::Health {
                max: Some(40f32),
                min: None,
                value: crate::types::RangeOrVal::Fixed(40f32),
            },
            hurt_on_condition: super::super::components::HurtOnCondition {
                damage_conditions: None,
            },
            is_hidden_when_invisible: super::super::components::IsHiddenWhenInvisible,
            jump_static: super::super::components::JumpStatic {
                jump_power: Some(0.42f32),
            },
            looked_at: super::super::components::LookedAt {
                field_of_view: Some(26f32),
                filters: Some(crate::types::BedrockValue::Object(
                    std::collections::HashMap::from([
                        (
                            "domain".to_string(),
                            crate::types::BedrockValue::String("head".to_string()),
                        ),
                        (
                            "operator".to_string(),
                            crate::types::BedrockValue::String("not".to_string()),
                        ),
                        (
                            "subject".to_string(),
                            crate::types::BedrockValue::String("other".to_string()),
                        ),
                        (
                            "test".to_string(),
                            crate::types::BedrockValue::String("has_equipment".to_string()),
                        ),
                        (
                            "value".to_string(),
                            crate::types::BedrockValue::String("carved_pumpkin".to_string()),
                        ),
                    ]),
                )),
                find_players_only: Some(true),
                line_of_sight_obstruction_type: Some("collision".to_string()),
                look_at_locations: None,
                looked_at_cooldown: None,
                looked_at_event: None,
                min_looked_at_duration: Some(0.25f32),
                not_looked_at_event: None,
                scale_fov_by_distance: Some(true),
                search_radius: Some(64f32),
                set_target: Some("once_and_stop_scanning".to_string()),
            },
            loot: super::super::components::Loot {
                table: "loot_tables/entities/enderman.json".to_string(),
            },
            movement_basic: super::super::components::MovementBasic {
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
                can_path_over_water: Some(false),
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
            renders_when_invisible: super::super::components::RendersWhenInvisible,
            teleport: super::super::components::Teleport {
                dark_teleport_chance: Some(0.01f32),
                light_teleport_chance: Some(0.05f32),
                max_random_teleport_time: Some(30f32),
                min_random_teleport_time: Some(0f32),
                random_teleport_cube: Some(vec![32f32, 16f32, 32f32]),
                random_teleports: Some(true),
                target_distance: Some(16f32),
                target_teleport_chance: Some(0.05f32),
            },
            type_family: super::super::components::TypeFamily {
                family: vec![
                    "enderman".to_string(),
                    "monster".to_string(),
                    "mob".to_string(),
                ],
            },
            variable_max_auto_step: super::super::components::VariableMaxAutoStep {
                base_value: Some(1.0625f32),
                controlled_value: Some(0.5625f32),
                jump_prevented_value: Some(0.5625f32),
            },
        })
        .id()
}
/// Component groups for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EndermanComponentGroup {
    EndermanAngry,
    EndermanCalm,
    NotRiding,
    Riding,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EndermanEvent {
    BecomeAngry,
    EntitySpawned,
    OnCalm,
    StartedRiding,
    StoppedRiding,
}
