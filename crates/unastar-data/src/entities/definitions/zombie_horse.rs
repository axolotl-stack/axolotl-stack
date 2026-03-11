//! Generated definition for entity.
#[allow(unused_imports)]
use super::super::components::*;
use bevy_ecs::prelude::{Bundle, Commands, Entity};
/// Entity definition for `minecraft:zombie_horse`
pub struct ZombieHorse;
impl ZombieHorse {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:zombie_horse";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:zombie_horse`
#[derive(Bundle, Clone)]
pub struct ZombieHorseBundle {
    pub ambient_sound_interval: super::super::components::AmbientSoundInterval,
    pub balloonable: super::super::components::Balloonable,
    pub behavior_flee_sun: super::super::components::BehaviorFleeSun,
    pub behavior_float: super::super::components::BehaviorFloat,
    pub behavior_look_at_player: super::super::components::BehaviorLookAtPlayer,
    pub behavior_panic: super::super::components::BehaviorPanic,
    pub behavior_random_look_around: super::super::components::BehaviorRandomLookAround,
    pub behavior_random_stroll: super::super::components::BehaviorRandomStroll,
    pub behavior_tempt: super::super::components::BehaviorTempt,
    pub breathable: super::super::components::Breathable,
    pub burns_in_daylight: super::super::components::BurnsInDaylight,
    pub collision_box: super::super::components::CollisionBox,
    pub conditional_bandwidth_optimization:
        super::super::components::ConditionalBandwidthOptimization,
    pub despawn: super::super::components::Despawn,
    pub environment_sensor: super::super::components::EnvironmentSensor,
    pub equippable: super::super::components::Equippable,
    pub healable: super::super::components::Healable,
    pub health: super::super::components::Health,
    pub horse_jump_strength: super::super::components::HorseJumpStrength,
    pub hurt_on_condition: super::super::components::HurtOnCondition,
    pub is_hidden_when_invisible: super::super::components::IsHiddenWhenInvisible,
    pub jump_static: super::super::components::JumpStatic,
    pub movement: super::super::components::Movement,
    pub movement_basic: super::super::components::MovementBasic,
    pub nameable: super::super::components::Nameable,
    pub navigation_walk: super::super::components::NavigationWalk,
    pub physics: super::super::components::Physics,
    pub pushable: super::super::components::Pushable,
    pub type_family: super::super::components::TypeFamily,
}
/// Spawn a new `minecraft:zombie_horse` entity with default Bedrock components
pub fn spawn_zombie_horse(commands: &mut Commands) -> Entity {
    commands
        .spawn(ZombieHorseBundle {
            ambient_sound_interval: super::super::components::AmbientSoundInterval {
                event_name: Some("ambient".to_string()),
                event_names: None,
                range: Some(16f32),
                value: 8f32,
            },
            balloonable: super::super::components::Balloonable {
                mass: None,
                max_distance: None,
                on_balloon: None,
                on_unballoon: None,
                soft_distance: None,
            },
            behavior_flee_sun: super::super::components::BehaviorFleeSun {
                priority: Some(BehaviorFleeSunPriority {}),
                speed_multiplier: Some(BehaviorFleeSunSpeedMultiplier {}),
            },
            behavior_float: super::super::components::BehaviorFloat {
                chance_per_tick_to_float: Some(1f32),
                priority: Some(BehaviorFloatPriority {}),
                sink_with_passengers: Some(false),
                time_under_water_to_dismount_passengers: Some(2f32),
            },
            behavior_look_at_player: super::super::components::BehaviorLookAtPlayer {
                angle_of_view_horizontal: Some(360i32),
                angle_of_view_vertical: Some(360i32),
                look_distance: Some(6f32),
                look_time: None,
                priority: Some(BehaviorLookAtPlayerPriority {}),
                probability: Some(0.02f32),
                target_distance: None,
            },
            behavior_panic: super::super::components::BehaviorPanic {
                damage_sources: Some(vec![
                    "campfire".to_string(),
                    "fire".to_string(),
                    "freezing".to_string(),
                    "lava".to_string(),
                    "lightning".to_string(),
                    "magma".to_string(),
                    "soul_campfire".to_string(),
                    "temperature".to_string(),
                    "entity_attack".to_string(),
                    "entity_explosion".to_string(),
                    "fireworks".to_string(),
                    "magic".to_string(),
                    "projectile".to_string(),
                    "ram_attack".to_string(),
                    "sonic_boom".to_string(),
                    "wither".to_string(),
                    "mace_smash".to_string(),
                ]),
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
            behavior_tempt: super::super::components::BehaviorTempt {
                can_get_scared: Some(false),
                can_tempt_vertically: Some(false),
                can_tempt_while_ridden: Some(false),
                items: Some(vec![crate::types::BedrockValue::String(
                    "red_mushroom".to_string(),
                )]),
                on_end: None,
                on_start: None,
                priority: Some(BehaviorTemptPriority {}),
                sound_interval: None,
                speed_multiplier: Some(BehaviorTemptSpeedMultiplier {}),
                stop_distance: Some(1.5f32),
                tempt_sound: None,
                within_radius: Some(0f32),
            },
            breathable: super::super::components::Breathable {
                breathe_blocks: None,
                breathes_air: Some(true),
                breathes_lava: Some(false),
                breathes_solids: Some(false),
                breathes_water: Some(true),
                can_dehydrate: Some(false),
                generates_bubbles: Some(true),
                inhale_time: Some(0f32),
                non_breathe_blocks: None,
                suffocate_time: Some(0i32),
                total_supply: Some(15i32),
            },
            burns_in_daylight: super::super::components::BurnsInDaylight {
                value: crate::types::BedrockValue::Null,
            },
            collision_box: super::super::components::CollisionBox {
                height: Some(1.6f32),
                width: Some(1.4f32),
            },
            conditional_bandwidth_optimization:
                super::super::components::ConditionalBandwidthOptimization {
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
            environment_sensor: super::super::components::EnvironmentSensor {
                triggers: Some(crate::types::BedrockValue::Object(
                    std::collections::HashMap::from([
                        (
                            "event".to_string(),
                            crate::types::BedrockValue::String(
                                "minecraft:upgrade_to_1_21_130".to_string(),
                            ),
                        ),
                        (
                            "filters".to_string(),
                            crate::types::BedrockValue::Object(std::collections::HashMap::from([
                                (
                                    "domain".to_string(),
                                    crate::types::BedrockValue::String(
                                        "minecraft:was_upgraded_to_1_21_130".to_string(),
                                    ),
                                ),
                                (
                                    "operator".to_string(),
                                    crate::types::BedrockValue::String("!=".to_string()),
                                ),
                                (
                                    "test".to_string(),
                                    crate::types::BedrockValue::String("bool_property".to_string()),
                                ),
                            ])),
                        ),
                    ]),
                )),
            },
            equippable: super::super::components::Equippable {
                slots: Some(vec![
                    EquippableSlots {
                        accepted_items: Some(vec![crate::types::BedrockValue::String(
                            "saddle".to_string(),
                        )]),
                        interact_text: None,
                        item: Some(crate::types::BedrockValue::String("saddle".to_string())),
                        on_equip: Some(EquippableSlotsOnEquip {
                            event: Some("minecraft:horse_saddled".to_string()),
                            filters: None,
                            target: None,
                        }),
                        on_unequip: Some(EquippableSlotsOnUnequip {
                            event: Some("minecraft:horse_unsaddled".to_string()),
                            filters: None,
                            target: None,
                        }),
                        slot: Some(0i32),
                    },
                    EquippableSlots {
                        accepted_items: Some(vec![
                            crate::types::BedrockValue::String("horsearmorleather".to_string()),
                            crate::types::BedrockValue::String("horsearmoriron".to_string()),
                            crate::types::BedrockValue::String("horsearmorgold".to_string()),
                            crate::types::BedrockValue::String("horsearmordiamond".to_string()),
                            crate::types::BedrockValue::String(
                                "minecraft:copper_horse_armor".to_string(),
                            ),
                            crate::types::BedrockValue::String(
                                "minecraft:netherite_horse_armor".to_string(),
                            ),
                        ]),
                        interact_text: None,
                        item: Some(crate::types::BedrockValue::String(
                            "horsearmoriron".to_string(),
                        )),
                        on_equip: None,
                        on_unequip: None,
                        slot: Some(1i32),
                    },
                ]),
            },
            healable: super::super::components::Healable {
                filters: None,
                force_use: Some(false),
                items: Some(vec![HealableItems {
                    effects: None,
                    filters: None,
                    heal_amount: Some(3i32),
                    item: Some(crate::types::BedrockValue::String(
                        "red_mushroom".to_string(),
                    )),
                    result_item: None,
                }]),
            },
            health: super::super::components::Health {
                max: Some(25f32),
                min: None,
                value: crate::types::RangeOrVal::Fixed(25f32),
            },
            horse_jump_strength: super::super::components::HorseJumpStrength {
                value: crate::types::RangeOrVal::Range {
                    min: 0.5f32,
                    max: 0.7f32,
                },
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
                value: crate::types::RangeOrVal::Range {
                    min: 0.205f32,
                    max: 0.275f32,
                },
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
                avoid_damage_blocks: Some(true),
                avoid_portals: Some(false),
                avoid_sun: Some(true),
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
                is_amphibious: Some(true),
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
                    "zombiehorse".to_string(),
                    "undead".to_string(),
                    "mob".to_string(),
                ],
            },
        })
        .id()
}
/// Component groups for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ZombieHorseComponentGroup {
    HorseAdult,
    HorseBaby,
    HorseCanBeLeashed,
    HorseSaddled,
    HorseTamed,
    HorseWild,
    HorseWildWithRider,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ZombieHorseEvent {
    EntityBorn,
    EntitySpawned,
    HorseSaddled,
    HorseUnsaddled,
    HostileDismounted,
    HostileMounted,
    OnTame,
    SpawnAdult,
    SpawnAdultWithRider,
    SpawnTameAdult,
    UpgradeTo121130,
}
