//! Generated definition for entity.
#[allow(unused_imports)]
use super::super::components::*;
use bevy_ecs::prelude::{Bundle, Commands, Entity};
/// Entity definition for `minecraft:villager_v2`
pub struct VillagerV2;
impl VillagerV2 {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:villager_v2";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = false;
}
/// Component bundle for spawning a `minecraft:villager_v2`
#[derive(Bundle, Clone)]
pub struct VillagerV2Bundle {
    pub annotation_open_door: super::super::components::AnnotationOpenDoor,
    pub behavior_avoid_mob_type: super::super::components::BehaviorAvoidMobType,
    pub behavior_float: super::super::components::BehaviorFloat,
    pub behavior_hide: super::super::components::BehaviorHide,
    pub behavior_look_at_player: super::super::components::BehaviorLookAtPlayer,
    pub behavior_look_at_trading_player: super::super::components::BehaviorLookAtTradingPlayer,
    pub behavior_move_indoors: super::super::components::BehaviorMoveIndoors,
    pub behavior_move_towards_dwelling_restriction:
        super::super::components::BehaviorMoveTowardsDwellingRestriction,
    pub behavior_panic: super::super::components::BehaviorPanic,
    pub behavior_pickup_items: super::super::components::BehaviorPickupItems,
    pub behavior_random_stroll: super::super::components::BehaviorRandomStroll,
    pub behavior_share_items: super::super::components::BehaviorShareItems,
    pub behavior_trade_with_player: super::super::components::BehaviorTradeWithPlayer,
    pub breathable: super::super::components::Breathable,
    pub can_climb: super::super::components::CanClimb,
    pub collision_box: super::super::components::CollisionBox,
    pub conditional_bandwidth_optimization:
        super::super::components::ConditionalBandwidthOptimization,
    pub damage_sensor: super::super::components::DamageSensor,
    pub dweller: super::super::components::Dweller,
    pub equipment: super::super::components::Equipment,
    pub follow_range: super::super::components::FollowRange,
    pub health: super::super::components::Health,
    pub hide: super::super::components::Hide,
    pub hurt_on_condition: super::super::components::HurtOnCondition,
    pub inventory: super::super::components::Inventory,
    pub is_hidden_when_invisible: super::super::components::IsHiddenWhenInvisible,
    pub jump_static: super::super::components::JumpStatic,
    pub mark_variant: super::super::components::MarkVariant,
    pub movement: super::super::components::Movement,
    pub movement_basic: super::super::components::MovementBasic,
    pub nameable: super::super::components::Nameable,
    pub navigation_walk: super::super::components::NavigationWalk,
    pub persistent: super::super::components::Persistent,
    pub physics: super::super::components::Physics,
    pub pushable: super::super::components::Pushable,
    pub type_family: super::super::components::TypeFamily,
}
/// Spawn a new `minecraft:villager_v2` entity with default Bedrock components
pub fn spawn_villager_v2(commands: &mut Commands) -> Entity {
    commands
        .spawn(VillagerV2Bundle {
            annotation_open_door: super::super::components::AnnotationOpenDoor,
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
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("any_of"
                        .to_string(), crate ::types::BedrockValue::Array(vec![crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("subject"
                        .to_string(), crate ::types::BedrockValue::String("other"
                        .to_string())), ("test".to_string(), crate
                        ::types::BedrockValue::String("is_family".to_string())), ("value"
                        .to_string(), crate ::types::BedrockValue::String("zombie"
                        .to_string()))])), crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("subject"
                        .to_string(), crate ::types::BedrockValue::String("other"
                        .to_string())), ("test".to_string(), crate
                        ::types::BedrockValue::String("is_family".to_string())), ("value"
                        .to_string(), crate
                        ::types::BedrockValue::String("zombie_villager".to_string()))])),
                        crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("subject"
                        .to_string(), crate ::types::BedrockValue::String("other"
                        .to_string())), ("test".to_string(), crate
                        ::types::BedrockValue::String("is_family".to_string())), ("value"
                        .to_string(), crate ::types::BedrockValue::String("illager"
                        .to_string()))])), crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("subject"
                        .to_string(), crate ::types::BedrockValue::String("other"
                        .to_string())), ("test".to_string(), crate
                        ::types::BedrockValue::String("is_family".to_string())), ("value"
                        .to_string(), crate ::types::BedrockValue::String("vex"
                        .to_string()))])), crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("subject"
                        .to_string(), crate ::types::BedrockValue::String("other"
                        .to_string())), ("test".to_string(), crate
                        ::types::BedrockValue::String("is_family".to_string())), ("value"
                        .to_string(), crate ::types::BedrockValue::String("zoglin"
                        .to_string()))]))]))]))), max_dist : Some(8f32), max_flee : None,
                        max_height : None, must_see : None, must_see_forget_duration :
                        None, priority : None, reevaluate_description : None,
                        sprint_speed_multiplier : Some(0.6f32), walk_speed_multiplier :
                        Some(0.6f32), within_default : None }
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
            behavior_float: super::super::components::BehaviorFloat {
                chance_per_tick_to_float: Some(0.8f32),
                priority: Some(BehaviorFloatPriority {}),
                sink_with_passengers: Some(false),
                time_under_water_to_dismount_passengers: Some(0f32),
            },
            behavior_hide: super::super::components::BehaviorHide {
                duration: Some(30f32),
                poi_type: Some("bed".to_string()),
                priority: Some(BehaviorHidePriority {}),
                speed_multiplier: Some(BehaviorHideSpeedMultiplier {}),
                timeout_cooldown: Some(8f32),
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
            behavior_look_at_trading_player: super::super::components::BehaviorLookAtTradingPlayer {
                angle_of_view_horizontal: Some(360i32),
                angle_of_view_vertical: Some(360i32),
                look_distance: Some(8f32),
                look_time: None,
                priority: Some(BehaviorLookAtTradingPlayerPriority {
                }),
                probability: Some(0.02f32),
            },
            behavior_move_indoors: super::super::components::BehaviorMoveIndoors {
                priority: Some(BehaviorMoveIndoorsPriority {}),
                speed_multiplier: Some(BehaviorMoveIndoorsSpeedMultiplier {
                }),
                timeout_cooldown: Some(8f32),
            },
            behavior_move_towards_dwelling_restriction: super::super::components::BehaviorMoveTowardsDwellingRestriction {
                priority: Some(BehaviorMoveTowardsDwellingRestrictionPriority {
                }),
                speed_multiplier: Some(BehaviorMoveTowardsDwellingRestrictionSpeedMultiplier {}),
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
            behavior_pickup_items: super::super::components::BehaviorPickupItems {
                can_pickup_any_item: Some(false),
                can_pickup_to_hand_or_equipment: Some(false),
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
            behavior_share_items: super::super::components::BehaviorShareItems {
                entity_types: Some(
                    vec![
                        BehaviorShareItemsEntityTypes { check_if_outnumbered : None,
                        cooldown : None, filters : Some(crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("subject"
                        .to_string(), crate ::types::BedrockValue::String("other"
                        .to_string())), ("test".to_string(), crate
                        ::types::BedrockValue::String("is_family".to_string())), ("value"
                        .to_string(), crate ::types::BedrockValue::String("villager"
                        .to_string()))]))), max_dist : None, max_flee : None, max_height
                        : None, must_see : None, must_see_forget_duration : None,
                        priority : None, reevaluate_description : None,
                        sprint_speed_multiplier : None, walk_speed_multiplier : None,
                        within_default : None }
                    ],
                ),
                goal_radius: Some(2f32),
                max_dist: Some(3f32),
                priority: Some(BehaviorShareItemsPriority {}),
                speed_multiplier: Some(BehaviorShareItemsSpeedMultiplier {
                }),
            },
            behavior_trade_with_player: super::super::components::BehaviorTradeWithPlayer {
                filters: Some(
                    crate::types::BedrockValue::Object(
                        std::collections::HashMap::from([
                            (
                                "all_of".to_string(),
                                crate::types::BedrockValue::Array(
                                    vec![
                                        crate
                                        ::types::BedrockValue::Object(std::collections::HashMap::from([("filter"
                                        .to_string(), crate
                                        ::types::BedrockValue::Object(std::collections::HashMap::from([("test"
                                        .to_string(), crate ::types::BedrockValue::String("in_water"
                                        .to_string())), ("value".to_string(), crate
                                        ::types::BedrockValue::Bool(false))])))])), crate
                                        ::types::BedrockValue::Object(std::collections::HashMap::from([("any_of"
                                        .to_string(), crate ::types::BedrockValue::Array(vec![crate
                                        ::types::BedrockValue::Object(std::collections::HashMap::from([("test"
                                        .to_string(), crate
                                        ::types::BedrockValue::String("on_ground".to_string())),
                                        ("value".to_string(), crate
                                        ::types::BedrockValue::Bool(true))])), crate
                                        ::types::BedrockValue::Object(std::collections::HashMap::from([("test"
                                        .to_string(), crate
                                        ::types::BedrockValue::String("is_sleeping".to_string())),
                                        ("value".to_string(), crate
                                        ::types::BedrockValue::Bool(true))]))]))]))
                                    ],
                                ),
                            ),
                        ]),
                    ),
                ),
                priority: Some(BehaviorTradeWithPlayerPriority {}),
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
                height: Some(1.9f32),
                width: Some(0.6f32),
            },
            conditional_bandwidth_optimization: super::super::components::ConditionalBandwidthOptimization {
                conditional_values: None,
                default_values: None,
            },
            damage_sensor: super::super::components::DamageSensor {
                triggers: Some(
                    vec![
                        DamageSensorTriggers { cause : None, damage_modifier : None,
                        damage_multiplier : None, deals_damage : Some("false"
                        .to_string()), on_damage : Some(crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("event"
                        .to_string(), crate ::types::BedrockValue::String("become_witch"
                        .to_string())), ("filter".to_string(), crate
                        ::types::BedrockValue::Array(vec![crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("subject"
                        .to_string(), crate ::types::BedrockValue::String("other"
                        .to_string())), ("test".to_string(), crate
                        ::types::BedrockValue::String("is_family".to_string())), ("value"
                        .to_string(), crate ::types::BedrockValue::String("lightning"
                        .to_string()))])), crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("operator"
                        .to_string(), crate ::types::BedrockValue::String("!="
                        .to_string())), ("test".to_string(), crate
                        ::types::BedrockValue::String("is_difficulty".to_string())),
                        ("value".to_string(), crate
                        ::types::BedrockValue::String("peaceful".to_string()))]))]))]))),
                        on_damage_sound_event : None }, DamageSensorTriggers { cause :
                        None, damage_modifier : None, damage_multiplier : None,
                        deals_damage : None, on_damage : Some(crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("event"
                        .to_string(), crate ::types::BedrockValue::String("become_zombie"
                        .to_string())), ("filters".to_string(), crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("all_of"
                        .to_string(), crate ::types::BedrockValue::Array(vec![crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("test"
                        .to_string(), crate ::types::BedrockValue::String("has_damage"
                        .to_string())), ("value".to_string(), crate
                        ::types::BedrockValue::String("fatal"
                        .to_string()))]))]))])))]))), on_damage_sound_event : None }
                    ],
                ),
            },
            dweller: super::super::components::Dweller {
                can_find_poi: Some(true),
                can_migrate: Some(true),
                dweller_role: Some("inhabitant".to_string()),
                dwelling_bounds_tolerance: None,
                dwelling_type: Some("village".to_string()),
                first_founding_reward: Some(5i32),
                preferred_profession: None,
                update_interval_base: Some(60f32),
                update_interval_variant: Some(40f32),
            },
            equipment: super::super::components::Equipment {
                slot_drop_chance: None,
                table: None,
            },
            follow_range: super::super::components::FollowRange {
                max: None,
                min: None,
                value: crate::types::RangeOrVal::Fixed(128f32),
            },
            health: super::super::components::Health {
                max: Some(20f32),
                min: None,
                value: crate::types::RangeOrVal::Fixed(20f32),
            },
            hide: super::super::components::Hide,
            hurt_on_condition: super::super::components::HurtOnCondition {
                damage_conditions: None,
            },
            inventory: super::super::components::Inventory {
                additional_slots_per_strength: Some(0i32),
                can_be_siphoned_from: Some(false),
                container_type: Some("none".to_string()),
                inventory_size: Some(8i32),
                private: Some(true),
                restrict_to_owner: Some(false),
            },
            is_hidden_when_invisible: super::super::components::IsHiddenWhenInvisible,
            jump_static: super::super::components::JumpStatic {
                jump_power: Some(0.42f32),
            },
            mark_variant: super::super::components::MarkVariant {
                value: 0i32,
            },
            movement: super::super::components::Movement {
                max: None,
                min: None,
                value: crate::types::RangeOrVal::Fixed(0.5f32),
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
                can_open_doors: Some(true),
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
            persistent: super::super::components::Persistent,
            physics: super::super::components::Physics {
                has_collision: Some(true),
                has_gravity: Some(true),
                push_towards_closest_space: Some(false),
            },
            pushable: super::super::components::Pushable {
                is_pushable: Some(true),
                is_pushable_by_piston: Some(true),
            },
            type_family: super::super::components::TypeFamily {
                family: vec!["villager".to_string(), "mob".to_string()],
            },
        })
        .id()
}
/// Component groups for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VillagerV2ComponentGroup {
    Adult,
    Armorer,
    Baby,
    BasicSchedule,
    BecomeWitch,
    BecomeZombie,
    BedScheduleVillager,
    BehaviorNonPeasant,
    BehaviorPeasant,
    Butcher,
    Cartographer,
    ChildSchedule,
    Cleric,
    DesertVillager,
    Farmer,
    FarmerSchedule,
    FisherSchedule,
    Fisherman,
    Fletcher,
    GatherScheduleVillager,
    HomeScheduleVillager,
    JobSpecificGoals,
    JoblessSchedule,
    JungleVillager,
    Leatherworker,
    Librarian,
    LibrarianSchedule,
    MakeAndReceiveLove,
    Mason,
    Celebrate,
    Nitwit,
    PlayScheduleVillager,
    SavannaVillager,
    Shepherd,
    SnowVillager,
    SwampVillager,
    TaigaVillager,
    Toolsmith,
    TradeComponents,
    TradeResupplyComponentGroup,
    Unskilled,
    VillagerSkin0,
    VillagerSkin1,
    VillagerSkin2,
    VillagerSkin3,
    VillagerSkin4,
    VillagerSkin5,
    WanderScheduleVillager,
    Weaponsmith,
    WorkSchedule,
    WorkScheduleFarmer,
    WorkScheduleFisher,
    WorkScheduleLibrarian,
    WorkScheduleVillager,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VillagerV2Event {
    BecomeWitch,
    BecomeZombie,
    AgeableGrowUp,
    BecomeArmorer,
    BecomeButcher,
    BecomeCartographer,
    BecomeCleric,
    BecomeFarmer,
    BecomeFisherman,
    BecomeFletcher,
    BecomeLeatherworker,
    BecomeLibrarian,
    BecomeMason,
    BecomeSheperd,
    BecomeToolsmith,
    BecomeUnskilled,
    BecomeWeaponsmith,
    EntityBorn,
    EntitySpawned,
    EntityTransformed,
    ResupplyTrades,
    ScheduleBedVillager,
    ScheduleGatherVillager,
    ScheduleHomeVillager,
    SchedulePlayVillager,
    ScheduleWanderVillager,
    ScheduleWorkFarmer,
    ScheduleWorkFisher,
    ScheduleWorkLibrarian,
    ScheduleWorkProVillager,
    SpawnArmorer,
    SpawnButcher,
    SpawnCleric,
    SpawnFarmer,
    SpawnFromVillage,
    SpawnLibrarian,
    StartCelebrating,
    StopCelebrating,
}
