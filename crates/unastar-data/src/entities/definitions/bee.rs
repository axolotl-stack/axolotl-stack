//! Generated definition for entity.
#[allow(unused_imports)]
use super::super::components::*;
use bevy_ecs::prelude::{Bundle, Commands, Entity};
/// Entity definition for `minecraft:bee`
pub struct Bee;
impl Bee {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:bee";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:bee`
#[derive(Bundle, Clone)]
pub struct BeeBundle {
    pub balloonable: super::super::components::Balloonable,
    pub behavior_float: super::super::components::BehaviorFloat,
    pub behavior_move_towards_home_restriction:
        super::super::components::BehaviorMoveTowardsHomeRestriction,
    pub behavior_random_hover: super::super::components::BehaviorRandomHover,
    pub behavior_tempt: super::super::components::BehaviorTempt,
    pub block_sensor: super::super::components::BlockSensor,
    pub breathable: super::super::components::Breathable,
    pub can_fly: super::super::components::CanFly,
    pub collision_box: super::super::components::CollisionBox,
    pub conditional_bandwidth_optimization:
        super::super::components::ConditionalBandwidthOptimization,
    pub damage_sensor: super::super::components::DamageSensor,
    pub flying_speed: super::super::components::FlyingSpeed,
    pub follow_range: super::super::components::FollowRange,
    pub game_event_movement_tracking: super::super::components::GameEventMovementTracking,
    pub health: super::super::components::Health,
    pub home: super::super::components::Home,
    pub hurt_on_condition: super::super::components::HurtOnCondition,
    pub interact: super::super::components::Interact,
    pub is_hidden_when_invisible: super::super::components::IsHiddenWhenInvisible,
    pub jump_static: super::super::components::JumpStatic,
    pub leashable: super::super::components::Leashable,
    pub movement: super::super::components::Movement,
    pub movement_hover: super::super::components::MovementHover,
    pub nameable: super::super::components::Nameable,
    pub navigation_hover: super::super::components::NavigationHover,
    pub on_target_acquired: super::super::components::OnTargetAcquired,
    pub physics: super::super::components::Physics,
    pub pushable: super::super::components::Pushable,
    pub type_family: super::super::components::TypeFamily,
}
/// Spawn a new `minecraft:bee` entity with default Bedrock components
pub fn spawn_bee(commands: &mut Commands) -> Entity {
    commands
        .spawn(BeeBundle {
            balloonable: super::super::components::Balloonable {
                mass: Some(0.5f32),
                max_distance: None,
                on_balloon: None,
                on_unballoon: None,
                soft_distance: None,
            },
            behavior_float: super::super::components::BehaviorFloat {
                chance_per_tick_to_float: Some(0.8f32),
                priority: Some(BehaviorFloatPriority {}),
                sink_with_passengers: Some(false),
                time_under_water_to_dismount_passengers: Some(0f32),
            },
            behavior_move_towards_home_restriction:
                super::super::components::BehaviorMoveTowardsHomeRestriction {
                    priority: Some(BehaviorMoveTowardsHomeRestrictionPriority {}),
                    speed_multiplier: Some(BehaviorMoveTowardsHomeRestrictionSpeedMultiplier {}),
                },
            behavior_random_hover: super::super::components::BehaviorRandomHover {
                hover_height: None,
                interval: Some(1i32),
                priority: Some(BehaviorRandomHoverPriority {}),
                speed_multiplier: Some(BehaviorRandomHoverSpeedMultiplier {}),
                xz_dist: Some(8i32),
                y_dist: Some(8i32),
                y_offset: Some(-1f32),
            },
            behavior_tempt: super::super::components::BehaviorTempt {
                can_get_scared: Some(false),
                can_tempt_vertically: Some(true),
                can_tempt_while_ridden: Some(false),
                items: Some(vec![
                    crate::types::BedrockValue::String("minecraft:poppy".to_string()),
                    crate::types::BedrockValue::String("minecraft:blue_orchid".to_string()),
                    crate::types::BedrockValue::String("minecraft:allium".to_string()),
                    crate::types::BedrockValue::String("minecraft:azure_bluet".to_string()),
                    crate::types::BedrockValue::String("minecraft:red_tulip".to_string()),
                    crate::types::BedrockValue::String("minecraft:orange_tulip".to_string()),
                    crate::types::BedrockValue::String("minecraft:white_tulip".to_string()),
                    crate::types::BedrockValue::String("minecraft:pink_tulip".to_string()),
                    crate::types::BedrockValue::String("minecraft:oxeye_daisy".to_string()),
                    crate::types::BedrockValue::String("minecraft:cornflower".to_string()),
                    crate::types::BedrockValue::String("minecraft:lily_of_the_valley".to_string()),
                    crate::types::BedrockValue::String("minecraft:dandelion".to_string()),
                    crate::types::BedrockValue::String("minecraft:wither_rose".to_string()),
                    crate::types::BedrockValue::String("minecraft:sunflower".to_string()),
                    crate::types::BedrockValue::String("minecraft:lilac".to_string()),
                    crate::types::BedrockValue::String("minecraft:rose_bush".to_string()),
                    crate::types::BedrockValue::String("minecraft:peony".to_string()),
                    crate::types::BedrockValue::String("minecraft:flowering_azalea".to_string()),
                    crate::types::BedrockValue::String(
                        "minecraft:azalea_leaves_flowered".to_string(),
                    ),
                    crate::types::BedrockValue::String("minecraft:mangrove_propagule".to_string()),
                    crate::types::BedrockValue::String("minecraft:pitcher_plant".to_string()),
                    crate::types::BedrockValue::String("minecraft:torchflower".to_string()),
                    crate::types::BedrockValue::String("minecraft:cherry_leaves".to_string()),
                    crate::types::BedrockValue::String("minecraft:pink_petals".to_string()),
                    crate::types::BedrockValue::String("minecraft:open_eyeblossom".to_string()),
                    crate::types::BedrockValue::String("minecraft:wildflowers".to_string()),
                    crate::types::BedrockValue::String("minecraft:cactus_flower".to_string()),
                ]),
                on_end: None,
                on_start: None,
                priority: Some(BehaviorTemptPriority {}),
                sound_interval: None,
                speed_multiplier: Some(BehaviorTemptSpeedMultiplier {}),
                stop_distance: Some(1.5f32),
                tempt_sound: None,
                within_radius: Some(8f32),
            },
            block_sensor: super::super::components::BlockSensor {
                on_break: None,
                sensor_radius: Some(16i32),
                sources: Some(vec![crate::types::BedrockValue::Object(
                    std::collections::HashMap::from([
                        (
                            "subject".to_string(),
                            crate::types::BedrockValue::String("other".to_string()),
                        ),
                        (
                            "test".to_string(),
                            crate::types::BedrockValue::String("has_silk_touch".to_string()),
                        ),
                        ("value".to_string(), crate::types::BedrockValue::Bool(false)),
                    ]),
                )]),
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
                total_supply: Some(0i32),
            },
            can_fly: super::super::components::CanFly {
                value: crate::types::BedrockValue::Null,
            },
            collision_box: super::super::components::CollisionBox {
                height: Some(0.5f32),
                width: Some(0.55f32),
            },
            conditional_bandwidth_optimization:
                super::super::components::ConditionalBandwidthOptimization {
                    conditional_values: None,
                    default_values: None,
                },
            damage_sensor: super::super::components::DamageSensor {
                triggers: Some(vec![
                    DamageSensorTriggers {
                        cause: Some("fall".to_string()),
                        damage_modifier: None,
                        damage_multiplier: None,
                        deals_damage: Some("no".to_string()),
                        on_damage: None,
                        on_damage_sound_event: None,
                    },
                    DamageSensorTriggers {
                        cause: None,
                        damage_modifier: None,
                        damage_multiplier: None,
                        deals_damage: Some("no".to_string()),
                        on_damage: Some(crate::types::BedrockValue::Object(
                            std::collections::HashMap::from([(
                                "filters".to_string(),
                                crate::types::BedrockValue::Object(
                                    std::collections::HashMap::from([
                                        (
                                            "subject".to_string(),
                                            crate::types::BedrockValue::String("block".to_string()),
                                        ),
                                        (
                                            "test".to_string(),
                                            crate::types::BedrockValue::String(
                                                "is_block".to_string(),
                                            ),
                                        ),
                                        (
                                            "value".to_string(),
                                            crate::types::BedrockValue::String(
                                                "minecraft:sweet_berry_bush".to_string(),
                                            ),
                                        ),
                                    ]),
                                ),
                            )]),
                        )),
                        on_damage_sound_event: None,
                    },
                ]),
            },
            flying_speed: super::super::components::FlyingSpeed { value: 0.15f32 },
            follow_range: super::super::components::FollowRange {
                max: None,
                min: None,
                value: crate::types::RangeOrVal::Fixed(1024f32),
            },
            game_event_movement_tracking: super::super::components::GameEventMovementTracking {
                emit_flap: Some(true),
                emit_move: Some(true),
                emit_swim: Some(true),
            },
            health: super::super::components::Health {
                max: Some(10f32),
                min: None,
                value: crate::types::RangeOrVal::Fixed(10f32),
            },
            home: super::super::components::Home {
                home_block_list: None,
                restriction_radius: Some(22i32),
                restriction_type: Some("random_movement".to_string()),
            },
            hurt_on_condition: super::super::components::HurtOnCondition {
                damage_conditions: None,
            },
            interact: super::super::components::Interact {
                interactions: Some(vec![
                    InteractInteractions {
                        add_items: None,
                        admire: None,
                        barter: None,
                        cooldown: None,
                        cooldown_after_being_attacked: None,
                        drop_item_slot: None,
                        drop_item_y_offset: None,
                        equip_item_slot: None,
                        give_item: None,
                        health_amount: None,
                        hurt_item: None,
                        interact_text: Some("action.interact.feed".to_string()),
                        on_interact: Some(crate::types::BedrockValue::Object(
                            std::collections::HashMap::from([
                                (
                                    "event".to_string(),
                                    crate::types::BedrockValue::String(
                                        "fed_open_eyeblossom".to_string(),
                                    ),
                                ),
                                (
                                    "filters".to_string(),
                                    crate::types::BedrockValue::Object(
                                        std::collections::HashMap::from([(
                                            "all_of".to_string(),
                                            crate::types::BedrockValue::Array(vec![
                                                crate::types::BedrockValue::Object(
                                                    std::collections::HashMap::from([
                                                        (
                                                            "subject".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "other".to_string(),
                                                            ),
                                                        ),
                                                        (
                                                            "test".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "is_family".to_string(),
                                                            ),
                                                        ),
                                                        (
                                                            "value".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "player".to_string(),
                                                            ),
                                                        ),
                                                    ]),
                                                ),
                                                crate::types::BedrockValue::Object(
                                                    std::collections::HashMap::from([
                                                        (
                                                            "domain".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "hand".to_string(),
                                                            ),
                                                        ),
                                                        (
                                                            "subject".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "other".to_string(),
                                                            ),
                                                        ),
                                                        (
                                                            "test".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "has_equipment".to_string(),
                                                            ),
                                                        ),
                                                        (
                                                            "value".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "minecraft:open_eyeblossom"
                                                                    .to_string(),
                                                            ),
                                                        ),
                                                    ]),
                                                ),
                                            ]),
                                        )]),
                                    ),
                                ),
                            ]),
                        )),
                        particle_on_start: Some(InteractInteractionsParticleOnStart {
                            particle_offset_towards_interactor: None,
                            particle_type: Some("food".to_string()),
                            particle_y_offset: None,
                        }),
                        play_sounds: None,
                        repair_entity_item: None,
                        spawn_entities: None,
                        spawn_items: None,
                        swing: None,
                        take_item: None,
                        transform_to_item: None,
                        use_item: Some(true),
                        vibration: None,
                    },
                    InteractInteractions {
                        add_items: None,
                        admire: None,
                        barter: None,
                        cooldown: None,
                        cooldown_after_being_attacked: None,
                        drop_item_slot: None,
                        drop_item_y_offset: None,
                        equip_item_slot: None,
                        give_item: None,
                        health_amount: None,
                        hurt_item: None,
                        interact_text: Some("action.interact.feed".to_string()),
                        on_interact: Some(crate::types::BedrockValue::Object(
                            std::collections::HashMap::from([
                                (
                                    "event".to_string(),
                                    crate::types::BedrockValue::String(
                                        "fed_wither_rose".to_string(),
                                    ),
                                ),
                                (
                                    "filters".to_string(),
                                    crate::types::BedrockValue::Object(
                                        std::collections::HashMap::from([(
                                            "all_of".to_string(),
                                            crate::types::BedrockValue::Array(vec![
                                                crate::types::BedrockValue::Object(
                                                    std::collections::HashMap::from([
                                                        (
                                                            "subject".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "other".to_string(),
                                                            ),
                                                        ),
                                                        (
                                                            "test".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "is_family".to_string(),
                                                            ),
                                                        ),
                                                        (
                                                            "value".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "player".to_string(),
                                                            ),
                                                        ),
                                                    ]),
                                                ),
                                                crate::types::BedrockValue::Object(
                                                    std::collections::HashMap::from([
                                                        (
                                                            "domain".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "hand".to_string(),
                                                            ),
                                                        ),
                                                        (
                                                            "subject".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "other".to_string(),
                                                            ),
                                                        ),
                                                        (
                                                            "test".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "has_equipment".to_string(),
                                                            ),
                                                        ),
                                                        (
                                                            "value".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "minecraft:wither_rose".to_string(),
                                                            ),
                                                        ),
                                                    ]),
                                                ),
                                            ]),
                                        )]),
                                    ),
                                ),
                            ]),
                        )),
                        particle_on_start: Some(InteractInteractionsParticleOnStart {
                            particle_offset_towards_interactor: None,
                            particle_type: Some("food".to_string()),
                            particle_y_offset: None,
                        }),
                        play_sounds: None,
                        repair_entity_item: None,
                        spawn_entities: None,
                        spawn_items: None,
                        swing: None,
                        take_item: None,
                        transform_to_item: None,
                        use_item: Some(true),
                        vibration: None,
                    },
                ]),
            },
            is_hidden_when_invisible: super::super::components::IsHiddenWhenInvisible,
            jump_static: super::super::components::JumpStatic {
                jump_power: Some(0.42f32),
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
            movement_hover: super::super::components::MovementHover {
                max_turn: Some(30f32),
            },
            nameable: super::super::components::Nameable {
                allow_name_tag_renaming: Some(true),
                always_show: Some(false),
                default_trigger: None,
                name_actions: None,
            },
            navigation_hover: super::super::components::NavigationHover {
                avoid_damage_blocks: Some(true),
                avoid_portals: Some(false),
                avoid_sun: Some(false),
                avoid_water: Some(true),
                blocks_to_avoid: None,
                can_breach: Some(false),
                can_break_doors: Some(false),
                can_jump: Some(true),
                can_open_doors: Some(false),
                can_open_iron_doors: Some(false),
                can_pass_doors: Some(false),
                can_path_from_air: Some(true),
                can_path_over_lava: Some(false),
                can_path_over_water: Some(true),
                can_sink: Some(false),
                can_swim: Some(false),
                can_walk: Some(true),
                can_walk_in_lava: Some(false),
                is_amphibious: Some(false),
            },
            on_target_acquired: super::super::components::OnTargetAcquired {
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
            type_family: super::super::components::TypeFamily {
                family: vec![
                    "bee".to_string(),
                    "mob".to_string(),
                    "arthropod".to_string(),
                ],
            },
        })
        .id()
}
/// Component groups for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BeeComponentGroup {
    AbortShelterDetection,
    AddPoisonEffect,
    AddWitherEffect,
    AngryBee,
    BeeAdult,
    BeeBaby,
    CountdownToPerish,
    DefaultSound,
    EasyAttack,
    EscapeFire,
    FindHive,
    HardAttack,
    HasNectar,
    HiveFull,
    LookForFood,
    NormalAttack,
    Perish,
    ReturnToHome,
    ShelterDetection,
    TakeNearestTarget,
    TrackAttacker,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BeeEvent {
    AbortSheltering,
    Attacked,
    CalmedDown,
    CollectedNectar,
    CountdownToPerishEvent,
    FedOpenEyeblossom,
    FedWitherRose,
    FindFlowerTimeout,
    FindHiveEvent,
    FindHiveTimeout,
    HiveDestroyed,
    AgeableGrowUp,
    EntityBorn,
    EntitySpawned,
    ExitedDisturbedHive,
    ExitedHive,
    ExitedHiveOnFire,
    HiveFull,
    SpawnAdult,
    OnPoisonEffectAdded,
    OnWitherEffectAdded,
    PerishEvent,
    SeekShelter,
    StopPanickingAfterFire,
}
