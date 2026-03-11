//! Generated definition for entity.
#[allow(unused_imports)]
use super::super::components::*;
use bevy_ecs::prelude::{Bundle, Commands, Entity};
/// Entity definition for `minecraft:vindicator`
pub struct Vindicator;
impl Vindicator {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:vindicator";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:vindicator`
#[derive(Bundle, Clone)]
pub struct VindicatorBundle {
    pub attack: super::super::components::Attack,
    pub behavior_avoid_mob_type: super::super::components::BehaviorAvoidMobType,
    pub behavior_equip_item: super::super::components::BehaviorEquipItem,
    pub behavior_float: super::super::components::BehaviorFloat,
    pub behavior_hurt_by_target: super::super::components::BehaviorHurtByTarget,
    pub behavior_look_at_player: super::super::components::BehaviorLookAtPlayer,
    pub behavior_melee_box_attack: super::super::components::BehaviorMeleeBoxAttack,
    pub behavior_pickup_items: super::super::components::BehaviorPickupItems,
    pub behavior_random_stroll: super::super::components::BehaviorRandomStroll,
    pub breathable: super::super::components::Breathable,
    pub can_join_raid: super::super::components::CanJoinRaid,
    pub collision_box: super::super::components::CollisionBox,
    pub conditional_bandwidth_optimization:
        super::super::components::ConditionalBandwidthOptimization,
    pub equip_item: super::super::components::EquipItem,
    pub equipment: super::super::components::Equipment,
    pub experience_reward: super::super::components::ExperienceReward,
    pub follow_range: super::super::components::FollowRange,
    pub health: super::super::components::Health,
    pub hurt_on_condition: super::super::components::HurtOnCondition,
    pub is_hidden_when_invisible: super::super::components::IsHiddenWhenInvisible,
    pub jump_static: super::super::components::JumpStatic,
    pub loot: super::super::components::Loot,
    pub movement: super::super::components::Movement,
    pub movement_basic: super::super::components::MovementBasic,
    pub nameable: super::super::components::Nameable,
    pub navigation_walk: super::super::components::NavigationWalk,
    pub on_target_acquired: super::super::components::OnTargetAcquired,
    pub on_target_escape: super::super::components::OnTargetEscape,
    pub physics: super::super::components::Physics,
    pub pushable: super::super::components::Pushable,
    pub shareables: super::super::components::Shareables,
    pub type_family: super::super::components::TypeFamily,
    pub variant: super::super::components::Variant,
}
/// Spawn a new `minecraft:vindicator` entity with default Bedrock components
pub fn spawn_vindicator(commands: &mut Commands) -> Entity {
    commands
        .spawn(VindicatorBundle {
            attack: super::super::components::Attack {
                damage: crate::types::RangeOrVal::Fixed(8f32),
                effect_duration: Some(crate::types::MolangOr::Value(0i32)),
                effect_name: None,
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
                        .to_string(), crate ::types::BedrockValue::String("creaking"
                        .to_string()))]))), max_dist : Some(8f32), max_flee : None,
                        max_height : None, must_see : None, must_see_forget_duration :
                        None, priority : None, reevaluate_description : None,
                        sprint_speed_multiplier : Some(1.2f32), walk_speed_multiplier :
                        None, within_default : None }
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
            behavior_equip_item: super::super::components::BehaviorEquipItem {
                priority: Some(BehaviorEquipItemPriority {}),
            },
            behavior_float: super::super::components::BehaviorFloat {
                chance_per_tick_to_float: Some(0.8f32),
                priority: Some(BehaviorFloatPriority {}),
                sink_with_passengers: Some(false),
                time_under_water_to_dismount_passengers: Some(0f32),
            },
            behavior_hurt_by_target: super::super::components::BehaviorHurtByTarget {
                alert_same_type: Some(false),
                entity_types: Some(
                    vec![
                        BehaviorHurtByTargetEntityTypes { check_if_outnumbered : None,
                        cooldown : None, filters : Some(crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("operator"
                        .to_string(), crate ::types::BedrockValue::String("!="
                        .to_string())), ("subject".to_string(), crate
                        ::types::BedrockValue::String("other".to_string())), ("test"
                        .to_string(), crate ::types::BedrockValue::String("is_family"
                        .to_string())), ("value".to_string(), crate
                        ::types::BedrockValue::String("illager".to_string()))]))),
                        max_dist : Some(64f32), max_flee : None, max_height : None,
                        must_see : None, must_see_forget_duration : None, priority :
                        None, reevaluate_description : None, sprint_speed_multiplier :
                        None, walk_speed_multiplier : None, within_default : None }
                    ],
                ),
                hurt_owner: Some(false),
                priority: Some(BehaviorHurtByTargetPriority {}),
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
            behavior_melee_box_attack: super::super::components::BehaviorMeleeBoxAttack {
                attack_once: Some(false),
                attack_types: None,
                can_spread_on_fire: Some(false),
                control_flags: None,
                cooldown_time: Some(1f32),
                horizontal_reach: Some(0.8f32),
                inner_boundary_time_increase: Some(0.25f32),
                max_dist: None,
                max_path_time: Some(0.55f32),
                melee_fov: Some(90f32),
                min_path_time: Some(0.2f32),
                on_attack: None,
                on_kill: None,
                outer_boundary_time_increase: Some(0.5f32),
                path_fail_time_increase: Some(0.75f32),
                path_inner_boundary: Some(16f32),
                path_outer_boundary: Some(32f32),
                priority: Some(BehaviorMeleeBoxAttackPriority {}),
                random_stop_interval: Some(0i32),
                reach_multiplier: None,
                require_complete_path: Some(false),
                set_persistent: None,
                speed_multiplier: Some(BehaviorMeleeBoxAttackSpeedMultiplier {
                }),
                target_dist: None,
                track_target: Some(false),
                x_max_rotation: Some(30f32),
                y_max_head_rotation: Some(30f32),
            },
            behavior_pickup_items: super::super::components::BehaviorPickupItems {
                can_pickup_any_item: Some(false),
                can_pickup_to_hand_or_equipment: Some(true),
                cooldown_after_being_attacked: None,
                excluded_items: None,
                goal_radius: Some(2f32),
                max_dist: Some(3f32),
                pickup_based_on_chance: Some(false),
                pickup_same_items_as_in_hand: None,
                priority: Some(BehaviorPickupItemsPriority {}),
                search_height: None,
                speed_multiplier: Some(BehaviorPickupItemsSpeedMultiplier {
                }),
                track_target: Some(false),
            },
            behavior_random_stroll: super::super::components::BehaviorRandomStroll {
                interval: Some(120i32),
                priority: Some(BehaviorRandomStrollPriority {}),
                speed_multiplier: Some(BehaviorRandomStrollSpeedMultiplier {
                }),
                xz_dist: Some(10i32),
                y_dist: Some(7i32),
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
            can_join_raid: super::super::components::CanJoinRaid,
            collision_box: super::super::components::CollisionBox {
                height: Some(1.9f32),
                width: Some(0.6f32),
            },
            conditional_bandwidth_optimization: super::super::components::ConditionalBandwidthOptimization {
                conditional_values: None,
                default_values: None,
            },
            equip_item: super::super::components::EquipItem {
                can_wear_armor: None,
                excluded_items: None,
            },
            equipment: super::super::components::Equipment {
                slot_drop_chance: None,
                table: Some("loot_tables/entities/vindicator_gear.json".to_string()),
            },
            experience_reward: super::super::components::ExperienceReward {
                on_bred: Some(crate::types::MolangOr::Value(0f32)),
                on_death: Some(
                    crate::types::MolangOr::Expr(
                        "query.last_hit_by_player ? (query.is_baby ? 12 : 5) + (Math.die_roll(query.equipment_count,1,3)) : 0"
                            .to_string(),
                    ),
                ),
            },
            follow_range: super::super::components::FollowRange {
                max: None,
                min: None,
                value: crate::types::RangeOrVal::Fixed(64f32),
            },
            health: super::super::components::Health {
                max: Some(24f32),
                min: None,
                value: crate::types::RangeOrVal::Fixed(24f32),
            },
            hurt_on_condition: super::super::components::HurtOnCondition {
                damage_conditions: None,
            },
            is_hidden_when_invisible: super::super::components::IsHiddenWhenInvisible,
            jump_static: super::super::components::JumpStatic {
                jump_power: Some(0.42f32),
            },
            loot: super::super::components::Loot {
                table: "loot_tables/entities/vindication_illager.json".to_string(),
            },
            movement: super::super::components::Movement {
                max: None,
                min: None,
                value: crate::types::RangeOrVal::Fixed(0.35f32),
            },
            movement_basic: super::super::components::MovementBasic {
                max_turn: Some(30f32),
            },
            nameable: super::super::components::Nameable {
                allow_name_tag_renaming: Some(true),
                always_show: Some(false),
                default_trigger: Some(
                    crate::types::BedrockValue::Object(
                        std::collections::HashMap::from([
                            (
                                "event".to_string(),
                                crate::types::BedrockValue::String(
                                    "minecraft:stop_johnny".to_string(),
                                ),
                            ),
                            (
                                "target".to_string(),
                                crate::types::BedrockValue::String("self".to_string()),
                            ),
                        ]),
                    ),
                ),
                name_actions: Some(
                    vec![
                        NameableNameActions { name_filter : Some("Johnny".to_string()),
                        on_named : Some(NameableNameActionsOnNamed { event :
                        Some("minecraft:start_johnny".to_string()), filters : None,
                        target : Some("self".to_string()) }) }
                    ],
                ),
            },
            navigation_walk: super::super::components::NavigationWalk {
                avoid_damage_blocks: Some(false),
                avoid_portals: Some(false),
                avoid_sun: Some(false),
                avoid_water: Some(false),
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
            on_target_acquired: super::super::components::OnTargetAcquired {
                value: crate::types::BedrockValue::Null,
            },
            on_target_escape: super::super::components::OnTargetEscape {
                value: crate::types::BedrockValue::Null,
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
            shareables: super::super::components::Shareables {
                all_items: Some(false),
                all_items_max_amount: Some(-1i32),
                all_items_surplus_amount: Some(-1i32),
                all_items_want_amount: Some(-1i32),
                items: Some(
                    vec![
                        ShareablesItems { admire : None, barter : None, consume_item :
                        None, craft_into : None, item : Some("minecraft:banner:15"
                        .to_string()), item_aux : None, max_amount : None, pickup_limit :
                        None, pickup_only : None, priority : Some(0i32),
                        stored_in_inventory : None, surplus_amount : Some(1i32),
                        want_amount : Some(1i32) }
                    ],
                ),
                singular_pickup: Some(false),
            },
            type_family: super::super::components::TypeFamily {
                family: vec![
                    "vindicator".to_string(), "monster".to_string(), "illager"
                    .to_string(), "mob".to_string()
                ],
            },
            variant: super::super::components::Variant {
                value: 0i32,
            },
        })
        .id()
}
/// Component groups for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VindicatorComponentGroup {
    Celebrate,
    DefaultTargeting,
    IllagerSquadCaptain,
    PatrolCaptain,
    PatrolFollower,
    RaidConfiguration,
    RaidDespawn,
    RaidPersistence,
    VindicatorAggro,
    VindicatorJohnny,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VindicatorEvent {
    BecomeAggro,
    EntitySpawned,
    PromoteToIllagerCaptain,
    PromoteToPatrolCaptain,
    RaidExpired,
    SpawnAsIllagerCaptain,
    SpawnAsPatrolFollower,
    SpawnForRaid,
    StartCelebrating,
    StartJohnny,
    StopAggro,
    StopCelebrating,
    StopJohnny,
}
