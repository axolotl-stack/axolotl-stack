//! Generated definition for entity.
#[allow(unused_imports)]
use super::super::components::*;
use bevy_ecs::prelude::{Bundle, Commands, Entity};
/// Entity definition for `minecraft:strider`
pub struct Strider;
impl Strider {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:strider";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:strider`
#[derive(Bundle, Clone)]
pub struct StriderBundle {
    pub attack: super::super::components::Attack,
    pub balloonable: super::super::components::Balloonable,
    pub behavior_look_at_player: super::super::components::BehaviorLookAtPlayer,
    pub behavior_panic: super::super::components::BehaviorPanic,
    pub behavior_random_look_around: super::super::components::BehaviorRandomLookAround,
    pub behavior_tempt: super::super::components::BehaviorTempt,
    pub breathable: super::super::components::Breathable,
    pub collision_box: super::super::components::CollisionBox,
    pub conditional_bandwidth_optimization:
        super::super::components::ConditionalBandwidthOptimization,
    pub despawn: super::super::components::Despawn,
    pub fire_immune: super::super::components::FireImmune,
    pub flying_speed: super::super::components::FlyingSpeed,
    pub follow_range: super::super::components::FollowRange,
    pub freezing_vulnerable: super::super::components::FreezingVulnerable,
    pub health: super::super::components::Health,
    pub hurt_on_condition: super::super::components::HurtOnCondition,
    pub inventory: super::super::components::Inventory,
    pub is_hidden_when_invisible: super::super::components::IsHiddenWhenInvisible,
    pub jump_static: super::super::components::JumpStatic,
    pub lava_movement: super::super::components::LavaMovement,
    pub leashable: super::super::components::Leashable,
    pub movement: super::super::components::Movement,
    pub movement_basic: super::super::components::MovementBasic,
    pub movement_sound_distance_offset: super::super::components::MovementSoundDistanceOffset,
    pub nameable: super::super::components::Nameable,
    pub navigation_walk: super::super::components::NavigationWalk,
    pub offspring: super::super::components::Offspring,
    pub physics: super::super::components::Physics,
    pub pushable: super::super::components::Pushable,
    pub pushable_by_block: super::super::components::PushableByBlock,
    pub pushable_by_entity: super::super::components::PushableByEntity,
    pub scale: super::super::components::Scale,
    pub spawn_egg_interaction: super::super::components::SpawnEggInteraction,
    pub type_family: super::super::components::TypeFamily,
}
/// Spawn a new `minecraft:strider` entity with default Bedrock components
pub fn spawn_strider(commands: &mut Commands) -> Entity {
    commands
        .spawn(StriderBundle {
            attack: super::super::components::Attack {
                damage: crate::types::RangeOrVal::Fixed(2f32),
                effect_duration: Some(crate::types::MolangOr::Value(0i32)),
                effect_name: None,
            },
            balloonable: super::super::components::Balloonable {
                mass: None,
                max_distance: None,
                on_balloon: None,
                on_unballoon: None,
                soft_distance: None,
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
                damage_sources: Some(
                    vec![
                        "[campfire, fire, fire_tick, freezing, lava, lightning, magma, soul_campfire, temperature, entity_attack, entity_explosion, fireworks, magic, projectile, ram_attack, sonic_boom, wither, mace_smash]"
                        .to_string()
                    ],
                ),
                force: Some(false),
                ignore_mob_damage: Some(false),
                panic_sound: Some("panic".to_string()),
                prefer_water: Some(false),
                priority: Some(BehaviorPanicPriority {}),
                sound_interval: Some(BehaviorPanicSoundInterval {
                    range_max: Some(3f32),
                    range_min: Some(1f32),
                }),
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
            behavior_tempt: super::super::components::BehaviorTempt {
                can_get_scared: Some(false),
                can_tempt_vertically: Some(false),
                can_tempt_while_ridden: Some(true),
                items: Some(
                    vec![
                        crate ::types::BedrockValue::String("warped_fungus".to_string()),
                        crate ::types::BedrockValue::String("warped_fungus_on_a_stick"
                        .to_string())
                    ],
                ),
                on_end: None,
                on_start: None,
                priority: Some(BehaviorTemptPriority {}),
                sound_interval: Some(
                    crate::types::BedrockValue::Object(
                        std::collections::HashMap::from([
                            (
                                "range_max".to_string(),
                                crate::types::BedrockValue::Float(5f64),
                            ),
                            (
                                "range_min".to_string(),
                                crate::types::BedrockValue::Float(2f64),
                            ),
                        ]),
                    ),
                ),
                speed_multiplier: Some(BehaviorTemptSpeedMultiplier {}),
                stop_distance: Some(1.5f32),
                tempt_sound: Some("tempt".to_string()),
                within_radius: Some(0f32),
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
                height: Some(1.7f32),
                width: Some(0.9f32),
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
            fire_immune: super::super::components::FireImmune,
            flying_speed: super::super::components::FlyingSpeed {
                value: 0.02f32,
            },
            follow_range: super::super::components::FollowRange {
                max: None,
                min: None,
                value: crate::types::RangeOrVal::Fixed(16f32),
            },
            freezing_vulnerable: super::super::components::FreezingVulnerable {
                data: None,
            },
            health: super::super::components::Health {
                max: Some(20f32),
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
            is_hidden_when_invisible: super::super::components::IsHiddenWhenInvisible,
            jump_static: super::super::components::JumpStatic {
                jump_power: Some(0.42f32),
            },
            lava_movement: super::super::components::LavaMovement {
                max: None,
                min: None,
                value: crate::types::RangeOrVal::Fixed(0.32f32),
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
                value: crate::types::RangeOrVal::Fixed(0.16f32),
            },
            movement_basic: super::super::components::MovementBasic {
                max_turn: Some(30f32),
            },
            movement_sound_distance_offset: super::super::components::MovementSoundDistanceOffset {
                value: 0.6f32,
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
                can_path_over_lava: Some(true),
                can_path_over_water: Some(false),
                can_sink: Some(false),
                can_swim: Some(false),
                can_walk: Some(true),
                can_walk_in_lava: Some(true),
                is_amphibious: Some(false),
            },
            offspring: super::super::components::Offspring {
                blend_attributes: None,
                combine_parent_colors: None,
                deny_parents_variant: None,
                inherit_tamed: None,
                mutation_factor: None,
                mutation_strategy: None,
                offspring_pairs: Some(OffspringOffspringPairs {}),
                parent_centric_attribute_blending: None,
                property_inheritance: None,
                random_extra_variant_mutation_interval: None,
                random_variant_mutation_interval: None,
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
            pushable_by_block: super::super::components::PushableByBlock {
                data: None,
            },
            pushable_by_entity: super::super::components::PushableByEntity {
                data: Some(
                    crate::types::BedrockValue::Object(
                        std::collections::HashMap::from([
                            (
                                "preset".to_string(),
                                crate::types::BedrockValue::Object(
                                    std::collections::HashMap::from([
                                        (
                                            "filters".to_string(),
                                            crate::types::BedrockValue::Object(
                                                std::collections::HashMap::from([
                                                    (
                                                        "all_of".to_string(),
                                                        crate::types::BedrockValue::Array(
                                                            vec![
                                                                crate
                                                                ::types::BedrockValue::Object(std::collections::HashMap::from([("subject"
                                                                .to_string(), crate ::types::BedrockValue::String("other"
                                                                .to_string())), ("test".to_string(), crate
                                                                ::types::BedrockValue::String("is_family".to_string())),
                                                                ("value".to_string(), crate
                                                                ::types::BedrockValue::String("sulfur_cube"
                                                                .to_string()))])), crate
                                                                ::types::BedrockValue::Object(std::collections::HashMap::from([("domain"
                                                                .to_string(), crate
                                                                ::types::BedrockValue::String("minecraft:sulfur_cube_archetype"
                                                                .to_string())), ("operator".to_string(), crate
                                                                ::types::BedrockValue::String("not".to_string())),
                                                                ("subject".to_string(), crate
                                                                ::types::BedrockValue::String("other".to_string())), ("test"
                                                                .to_string(), crate
                                                                ::types::BedrockValue::String("enum_property".to_string())),
                                                                ("value".to_string(), crate
                                                                ::types::BedrockValue::String("none".to_string()))])), crate
                                                                ::types::BedrockValue::Object(std::collections::HashMap::from([("subject"
                                                                .to_string(), crate ::types::BedrockValue::String("self"
                                                                .to_string())), ("test".to_string(), crate
                                                                ::types::BedrockValue::String("is_controlling_passenger_family"
                                                                .to_string())), ("value".to_string(), crate
                                                                ::types::BedrockValue::String("player".to_string()))]))
                                                            ],
                                                        ),
                                                    ),
                                                ]),
                                            ),
                                        ),
                                        (
                                            "push_mode".to_string(),
                                            crate::types::BedrockValue::String("none".to_string()),
                                        ),
                                        (
                                            "require_collision_overlap".to_string(),
                                            crate::types::BedrockValue::Bool(false),
                                        ),
                                    ]),
                                ),
                            ),
                        ]),
                    ),
                ),
            },
            scale: super::super::components::Scale {
                value: 1f32,
            },
            spawn_egg_interaction: super::super::components::SpawnEggInteraction {
                data: None,
            },
            type_family: super::super::components::TypeFamily {
                family: vec!["strider".to_string(), "mob".to_string()],
            },
        })
        .id()
}
/// Component groups for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StriderComponentGroup {
    DetectSuffocating,
    StartSuffocating,
    StriderAdult,
    StriderBaby,
    StriderParentJockey,
    StriderPathingBehaviors,
    StriderPiglinJockey,
    StriderSaddled,
    StriderUnsaddled,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StriderEvent {
    AgeableGrowUp,
    EntityBorn,
    EntitySpawned,
    OnSaddled,
    OnUnsaddled,
    SpawnBabyStriderJockey,
    OnNotRidingParent,
    SpawnAdult,
    SpawnAdultParentJockey,
    SpawnAdultPiglinJockey,
    SpawnBaby,
    StartSuffocating,
    StopSuffocating,
}
