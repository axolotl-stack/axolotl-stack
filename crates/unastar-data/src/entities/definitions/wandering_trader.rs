//! Generated definition for entity.
#[allow(unused_imports)]
use super::super::components::*;
use bevy_ecs::prelude::{Bundle, Commands, Entity};
/// Entity definition for `minecraft:wandering_trader`
pub struct WanderingTrader;
impl WanderingTrader {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:wandering_trader";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:wandering_trader`
#[derive(Bundle, Clone)]
pub struct WanderingTraderBundle {
    pub behavior_avoid_mob_type: super::super::components::BehaviorAvoidMobType,
    pub behavior_drink_milk: super::super::components::BehaviorDrinkMilk,
    pub behavior_drink_potion: super::super::components::BehaviorDrinkPotion,
    pub behavior_float: super::super::components::BehaviorFloat,
    pub behavior_look_at_player: super::super::components::BehaviorLookAtPlayer,
    pub behavior_look_at_trading_player: super::super::components::BehaviorLookAtTradingPlayer,
    pub behavior_move_towards_home_restriction:
        super::super::components::BehaviorMoveTowardsHomeRestriction,
    pub behavior_panic: super::super::components::BehaviorPanic,
    pub behavior_random_look_around: super::super::components::BehaviorRandomLookAround,
    pub behavior_random_stroll: super::super::components::BehaviorRandomStroll,
    pub behavior_trade_interest: super::super::components::BehaviorTradeInterest,
    pub behavior_trade_with_player: super::super::components::BehaviorTradeWithPlayer,
    pub breathable: super::super::components::Breathable,
    pub can_climb: super::super::components::CanClimb,
    pub collision_box: super::super::components::CollisionBox,
    pub conditional_bandwidth_optimization:
        super::super::components::ConditionalBandwidthOptimization,
    pub damage_sensor: super::super::components::DamageSensor,
    pub despawn: super::super::components::Despawn,
    pub economy_trade_table: super::super::components::EconomyTradeTable,
    pub health: super::super::components::Health,
    pub home: super::super::components::Home,
    pub hurt_on_condition: super::super::components::HurtOnCondition,
    pub is_hidden_when_invisible: super::super::components::IsHiddenWhenInvisible,
    pub jump_static: super::super::components::JumpStatic,
    pub movement: super::super::components::Movement,
    pub movement_basic: super::super::components::MovementBasic,
    pub nameable: super::super::components::Nameable,
    pub navigation_walk: super::super::components::NavigationWalk,
    pub physics: super::super::components::Physics,
    pub pushable: super::super::components::Pushable,
    pub spawn_entity: super::super::components::SpawnEntity,
    pub timer: super::super::components::Timer,
    pub type_family: super::super::components::TypeFamily,
}
/// Spawn a new `minecraft:wandering_trader` entity with default Bedrock components
pub fn spawn_wandering_trader(commands: &mut Commands) -> Entity {
    commands
        .spawn(WanderingTraderBundle {
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
                        .to_string(), crate ::types::BedrockValue::String("zombie_pigman"
                        .to_string()))])), crate
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
                        .to_string()))]))]))]))), max_dist : None, max_flee : None,
                        max_height : None, must_see : None, must_see_forget_duration :
                        None, priority : None, reevaluate_description : None,
                        sprint_speed_multiplier : Some(0.6f32), walk_speed_multiplier :
                        Some(0.6f32), within_default : None }
                    ],
                ),
                ignore_visibility: Some(false),
                ignore_visibilty: None,
                max_dist: Some(6f32),
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
            behavior_drink_milk: super::super::components::BehaviorDrinkMilk {
                control_flags: Some(BehaviorDrinkMilkControlFlags {}),
                cooldown_seconds: Some(5f32),
                filters: Some(
                    crate::types::BedrockValue::Object(
                        std::collections::HashMap::from([
                            (
                                "all_of".to_string(),
                                crate::types::BedrockValue::Array(
                                    vec![
                                        crate
                                        ::types::BedrockValue::Object(std::collections::HashMap::from([("test"
                                        .to_string(), crate
                                        ::types::BedrockValue::String("is_daytime".to_string())),
                                        ("value".to_string(), crate
                                        ::types::BedrockValue::Bool(true))])), crate
                                        ::types::BedrockValue::Object(std::collections::HashMap::from([("subject"
                                        .to_string(), crate ::types::BedrockValue::String("self"
                                        .to_string())), ("test".to_string(), crate
                                        ::types::BedrockValue::String("is_visible".to_string())),
                                        ("value".to_string(), crate
                                        ::types::BedrockValue::Bool(false))])), crate
                                        ::types::BedrockValue::Object(std::collections::HashMap::from([("subject"
                                        .to_string(), crate ::types::BedrockValue::String("self"
                                        .to_string())), ("test".to_string(), crate
                                        ::types::BedrockValue::String("is_avoiding_mobs"
                                        .to_string())), ("value".to_string(), crate
                                        ::types::BedrockValue::Bool(false))]))
                                    ],
                                ),
                            ),
                        ]),
                    ),
                ),
                priority: Some(BehaviorDrinkMilkPriority {}),
            },
            behavior_drink_potion: super::super::components::BehaviorDrinkPotion {
                potions: Some(
                    vec![
                        BehaviorDrinkPotionPotions { chance : 1f32, filters : crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("all_of"
                        .to_string(), crate ::types::BedrockValue::Array(vec![crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("any_of"
                        .to_string(), crate ::types::BedrockValue::Array(vec![crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("operator"
                        .to_string(), crate ::types::BedrockValue::String(">="
                        .to_string())), ("test".to_string(), crate
                        ::types::BedrockValue::String("hourly_clock_time".to_string())),
                        ("value".to_string(), crate
                        ::types::BedrockValue::Integer(18000i64))])), crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("operator"
                        .to_string(), crate ::types::BedrockValue::String("<"
                        .to_string())), ("test".to_string(), crate
                        ::types::BedrockValue::String("hourly_clock_time".to_string())),
                        ("value".to_string(), crate
                        ::types::BedrockValue::Integer(12000i64))]))]))])), crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("subject"
                        .to_string(), crate ::types::BedrockValue::String("self"
                        .to_string())), ("test".to_string(), crate
                        ::types::BedrockValue::String("is_visible".to_string())),
                        ("value".to_string(), crate
                        ::types::BedrockValue::Bool(true))])), crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("any_of"
                        .to_string(), crate ::types::BedrockValue::Array(vec![crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("subject"
                        .to_string(), crate ::types::BedrockValue::String("self"
                        .to_string())), ("test".to_string(), crate
                        ::types::BedrockValue::String("is_avoiding_mobs".to_string())),
                        ("value".to_string(), crate
                        ::types::BedrockValue::Bool(true))])), crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("filter"
                        .to_string(), crate ::types::BedrockValue::Array(vec![crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("subject"
                        .to_string(), crate ::types::BedrockValue::String("self"
                        .to_string())), ("test".to_string(), crate
                        ::types::BedrockValue::String("has_component".to_string())),
                        ("value".to_string(), crate
                        ::types::BedrockValue::String("minecraft:angry".to_string()))])),
                        crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("operator"
                        .to_string(), crate ::types::BedrockValue::String("!="
                        .to_string())), ("subject".to_string(), crate
                        ::types::BedrockValue::String("target".to_string())), ("test"
                        .to_string(), crate ::types::BedrockValue::String("is_family"
                        .to_string())), ("value".to_string(), crate
                        ::types::BedrockValue::String("player"
                        .to_string()))]))]))]))]))]))]))])), id : 7i32 },
                        BehaviorDrinkPotionPotions { chance : 1f32, filters : crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("all_of"
                        .to_string(), crate ::types::BedrockValue::Array(vec![crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("operator"
                        .to_string(), crate ::types::BedrockValue::String(">="
                        .to_string())), ("test".to_string(), crate
                        ::types::BedrockValue::String("hourly_clock_time".to_string())),
                        ("value".to_string(), crate
                        ::types::BedrockValue::Integer(12000i64))])), crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("operator"
                        .to_string(), crate ::types::BedrockValue::String("<"
                        .to_string())), ("test".to_string(), crate
                        ::types::BedrockValue::String("hourly_clock_time".to_string())),
                        ("value".to_string(), crate
                        ::types::BedrockValue::Integer(18000i64))])), crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("subject"
                        .to_string(), crate ::types::BedrockValue::String("self"
                        .to_string())), ("test".to_string(), crate
                        ::types::BedrockValue::String("is_visible".to_string())),
                        ("value".to_string(), crate
                        ::types::BedrockValue::Bool(true))])), crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("any_of"
                        .to_string(), crate ::types::BedrockValue::Array(vec![crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("subject"
                        .to_string(), crate ::types::BedrockValue::String("self"
                        .to_string())), ("test".to_string(), crate
                        ::types::BedrockValue::String("is_avoiding_mobs".to_string())),
                        ("value".to_string(), crate
                        ::types::BedrockValue::Bool(true))])), crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("subject"
                        .to_string(), crate ::types::BedrockValue::String("self"
                        .to_string())), ("test".to_string(), crate
                        ::types::BedrockValue::String("has_component".to_string())),
                        ("value".to_string(), crate
                        ::types::BedrockValue::String("minecraft:angry"
                        .to_string()))]))]))]))]))])), id : 8i32 }
                    ],
                ),
                priority: Some(BehaviorDrinkPotionPriority {}),
                speed_modifier: Some(crate::types::BedrockValue::Float(-0.2f64)),
                speed_multiplier: None,
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
            behavior_look_at_trading_player: super::super::components::BehaviorLookAtTradingPlayer {
                angle_of_view_horizontal: Some(360i32),
                angle_of_view_vertical: Some(360i32),
                look_distance: Some(8f32),
                look_time: None,
                priority: Some(BehaviorLookAtTradingPlayerPriority {
                }),
                probability: Some(0.02f32),
            },
            behavior_move_towards_home_restriction: super::super::components::BehaviorMoveTowardsHomeRestriction {
                priority: Some(BehaviorMoveTowardsHomeRestrictionPriority {
                }),
                speed_multiplier: Some(BehaviorMoveTowardsHomeRestrictionSpeedMultiplier {}),
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
            behavior_random_look_around: super::super::components::BehaviorRandomLookAround {
                angle_of_view_horizontal: None,
                angle_of_view_vertical: None,
                look_distance: None,
                look_time: None,
                priority: Some(BehaviorRandomLookAroundPriority {
                }),
                probability: None,
            },
            behavior_random_stroll: super::super::components::BehaviorRandomStroll {
                interval: Some(120i32),
                priority: Some(BehaviorRandomStrollPriority {}),
                speed_multiplier: Some(BehaviorRandomStrollSpeedMultiplier {
                }),
                xz_dist: Some(10i32),
                y_dist: Some(7i32),
            },
            behavior_trade_interest: super::super::components::BehaviorTradeInterest {
                carried_item_switch_time: Some(2f32),
                cooldown: Some(2f32),
                interest_time: Some(45f32),
                priority: Some(BehaviorTradeInterestPriority {}),
                remove_item_time: Some(1f32),
                within_radius: Some(6f32),
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
                        DamageSensorTriggers { cause : Some("entity_attack".to_string()),
                        damage_modifier : None, damage_multiplier : None, deals_damage :
                        Some("true".to_string()), on_damage : Some(crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("event"
                        .to_string(), crate
                        ::types::BedrockValue::String("minecraft:become_scared"
                        .to_string()))]))), on_damage_sound_event : None },
                        DamageSensorTriggers { cause : Some("projectile".to_string()),
                        damage_modifier : None, damage_multiplier : None, deals_damage :
                        Some("true".to_string()), on_damage : Some(crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("event"
                        .to_string(), crate
                        ::types::BedrockValue::String("minecraft:become_scared"
                        .to_string()))]))), on_damage_sound_event : None },
                        DamageSensorTriggers { cause : Some("magic".to_string()),
                        damage_modifier : None, damage_multiplier : None, deals_damage :
                        Some("true".to_string()), on_damage : Some(crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("event"
                        .to_string(), crate
                        ::types::BedrockValue::String("minecraft:become_scared"
                        .to_string()))]))), on_damage_sound_event : None }
                    ],
                ),
            },
            despawn: super::super::components::Despawn {
                despawn_from_chance: Some(true),
                despawn_from_distance: None,
                despawn_from_inactivity: Some(true),
                despawn_from_simulation_edge: Some(true),
                filters: Some(
                    crate::types::BedrockValue::Object(
                        std::collections::HashMap::from([
                            (
                                "all_of".to_string(),
                                crate::types::BedrockValue::Array(
                                    vec![
                                        crate
                                        ::types::BedrockValue::Object(std::collections::HashMap::from([("any_of"
                                        .to_string(), crate ::types::BedrockValue::Array(vec![crate
                                        ::types::BedrockValue::Object(std::collections::HashMap::from([("subject"
                                        .to_string(), crate ::types::BedrockValue::String("self"
                                        .to_string())), ("test".to_string(), crate
                                        ::types::BedrockValue::String("is_family".to_string())),
                                        ("value".to_string(), crate
                                        ::types::BedrockValue::String("wandering_trader_despawning"
                                        .to_string()))])), crate
                                        ::types::BedrockValue::Object(std::collections::HashMap::from([("subject"
                                        .to_string(), crate ::types::BedrockValue::String("self"
                                        .to_string())), ("test".to_string(), crate
                                        ::types::BedrockValue::String("has_trade_supply"
                                        .to_string())), ("value".to_string(), crate
                                        ::types::BedrockValue::Bool(false))]))]))])), crate
                                        ::types::BedrockValue::Object(std::collections::HashMap::from([("operator"
                                        .to_string(), crate ::types::BedrockValue::String(">"
                                        .to_string())), ("test".to_string(), crate
                                        ::types::BedrockValue::String("distance_to_nearest_player"
                                        .to_string())), ("value".to_string(), crate
                                        ::types::BedrockValue::Integer(24i64))]))
                                    ],
                                ),
                            ),
                        ]),
                    ),
                ),
                min_range_inactivity_timer: Some(30i32),
                min_range_random_chance: Some(800i32),
                remove_child_entities: Some(true),
            },
            economy_trade_table: super::super::components::EconomyTradeTable {
                convert_trades_economy: Some(false),
                cured_discount: None,
                display_name: Some("entity.wandering_trader.name".to_string()),
                hero_demand_discount: Some(-4i32),
                max_cured_discount: None,
                max_nearby_cured_discount: Some(-200i32),
                nearby_cured_discount: Some(-20i32),
                new_screen: Some(true),
                persist_trades: Some(false),
                show_trade_screen: Some(true),
                table: Some(
                    "trading/economy_trades/wandering_trader_trades.json".to_string(),
                ),
                use_legacy_price_formula: Some(false),
            },
            health: super::super::components::Health {
                max: Some(20f32),
                min: None,
                value: crate::types::RangeOrVal::Fixed(20f32),
            },
            home: super::super::components::Home {
                home_block_list: None,
                restriction_radius: Some(16i32),
                restriction_type: Some("none".to_string()),
            },
            hurt_on_condition: super::super::components::HurtOnCondition {
                damage_conditions: None,
            },
            is_hidden_when_invisible: super::super::components::IsHiddenWhenInvisible,
            jump_static: super::super::components::JumpStatic {
                jump_power: Some(0.42f32),
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
            spawn_entity: super::super::components::SpawnEntity {
                entities: Some(
                    vec![
                        SpawnEntityEntities { filters : None, max_wait_time : Some(0i32),
                        min_wait_time : Some(0i32), num_to_spawn : Some(2i32),
                        should_leash : Some(true), single_use : Some(true), spawn_entity
                        : Some("trader_llama".to_string()), spawn_event :
                        Some("minecraft:from_wandering_trader".to_string()), spawn_item :
                        None, spawn_item_event : None, spawn_method : None, spawn_sound :
                        None }
                    ],
                ),
            },
            timer: super::super::components::Timer {
                looping: Some(false),
                random_interval: Some(true),
                random_time_choices: Some(
                    vec![
                        TimerRandomTimeChoices { value : 2400i32, weight : Some(50i32) },
                        TimerRandomTimeChoices { value : 3600i32, weight : Some(50i32) }
                    ],
                ),
                time: None,
                time_down_event: Some(TimerTimeDownEvent {
                    event: Some("minecraft:start_despawn".to_string()),
                    filters: None,
                    target: Some("self".to_string()),
                }),
            },
            type_family: super::super::components::TypeFamily {
                family: vec!["wandering_trader".to_string(), "mob".to_string()],
            },
        })
        .id()
}
/// Component groups for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WanderingTraderComponentGroup {
    Despawning,
    Managed,
    Scared,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WanderingTraderEvent {
    BecomeCalm,
    BecomeScared,
    Scheduled,
    StartDespawn,
}
