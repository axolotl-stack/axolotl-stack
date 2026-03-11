//! Generated definition for entity.
#[allow(unused_imports)]
use super::super::components::*;
use bevy_ecs::prelude::{Bundle, Commands, Entity};
/// Entity definition for `minecraft:panda`
pub struct Panda;
impl Panda {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:panda";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:panda`
#[derive(Bundle, Clone)]
pub struct PandaBundle {
    pub balloonable: super::super::components::Balloonable,
    pub behavior_breed: super::super::components::BehaviorBreed,
    pub behavior_float: super::super::components::BehaviorFloat,
    pub behavior_hurt_by_target: super::super::components::BehaviorHurtByTarget,
    pub behavior_look_at_player: super::super::components::BehaviorLookAtPlayer,
    pub behavior_mount_pathing: super::super::components::BehaviorMountPathing,
    pub behavior_panic: super::super::components::BehaviorPanic,
    pub behavior_random_look_around: super::super::components::BehaviorRandomLookAround,
    pub behavior_random_sitting: super::super::components::BehaviorRandomSitting,
    pub behavior_random_stroll: super::super::components::BehaviorRandomStroll,
    pub behavior_snacking: super::super::components::BehaviorSnacking,
    pub behavior_tempt: super::super::components::BehaviorTempt,
    pub breathable: super::super::components::Breathable,
    pub can_climb: super::super::components::CanClimb,
    pub collision_box: super::super::components::CollisionBox,
    pub conditional_bandwidth_optimization:
        super::super::components::ConditionalBandwidthOptimization,
    pub despawn: super::super::components::Despawn,
    pub genetics: super::super::components::Genetics,
    pub giveable: super::super::components::Giveable,
    pub health: super::super::components::Health,
    pub hurt_on_condition: super::super::components::HurtOnCondition,
    pub inventory: super::super::components::Inventory,
    pub is_hidden_when_invisible: super::super::components::IsHiddenWhenInvisible,
    pub jump_static: super::super::components::JumpStatic,
    pub movement: super::super::components::Movement,
    pub movement_basic: super::super::components::MovementBasic,
    pub nameable: super::super::components::Nameable,
    pub navigation_walk: super::super::components::NavigationWalk,
    pub physics: super::super::components::Physics,
    pub pushable: super::super::components::Pushable,
    pub rideable: super::super::components::Rideable,
    pub scale: super::super::components::Scale,
    pub type_family: super::super::components::TypeFamily,
    pub variant: super::super::components::Variant,
    pub water_movement: super::super::components::WaterMovement,
}
/// Spawn a new `minecraft:panda` entity with default Bedrock components
pub fn spawn_panda(commands: &mut Commands) -> Entity {
    commands
        .spawn(PandaBundle {
            balloonable: super::super::components::Balloonable {
                mass: None,
                max_distance: None,
                on_balloon: None,
                on_unballoon: None,
                soft_distance: None,
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
            behavior_hurt_by_target: super::super::components::BehaviorHurtByTarget {
                alert_same_type: Some(false),
                entity_types: None,
                hurt_owner: Some(false),
                priority: Some(BehaviorHurtByTargetPriority {}),
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
            behavior_mount_pathing: super::super::components::BehaviorMountPathing {
                priority: Some(BehaviorMountPathingPriority {}),
                speed_multiplier: Some(BehaviorMountPathingSpeedMultiplier {
                }),
                target_dist: Some(0f32),
                track_target: Some(true),
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
            behavior_random_sitting: super::super::components::BehaviorRandomSitting {
                cooldown: Some(30f32),
                cooldown_time: Some(0f32),
                min_sit_time: Some(10f32),
                priority: Some(BehaviorRandomSittingPriority {}),
                speed_multiplier: None,
                start_chance: Some(0.01f32),
                stop_chance: Some(0.3f32),
            },
            behavior_random_stroll: super::super::components::BehaviorRandomStroll {
                interval: Some(120i32),
                priority: Some(BehaviorRandomStrollPriority {}),
                speed_multiplier: Some(BehaviorRandomStrollSpeedMultiplier {
                }),
                xz_dist: Some(10i32),
                y_dist: Some(7i32),
            },
            behavior_snacking: super::super::components::BehaviorSnacking {
                items: Some(
                    vec![
                        crate ::types::BedrockValue::String("bamboo".to_string()), crate
                        ::types::BedrockValue::String("cake".to_string())
                    ],
                ),
                priority: Some(BehaviorSnackingPriority {}),
                snacking_cooldown: Some(22.5f32),
                snacking_cooldown_min: Some(20f32),
                snacking_stop_chance: Some(0.001334f32),
            },
            behavior_tempt: super::super::components::BehaviorTempt {
                can_get_scared: Some(false),
                can_tempt_vertically: Some(false),
                can_tempt_while_ridden: Some(false),
                items: Some(
                    vec![crate ::types::BedrockValue::String("bamboo".to_string())],
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
                height: Some(1.25f32),
                width: Some(1.3f32),
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
            genetics: super::super::components::Genetics {
                genes: Some(
                    vec![
                        GeneticsGenes { allele_range : Some(crate
                        ::types::RangeOrVal::Range { min : 0f32, max : 15f32 }),
                        genetic_variants : Some(vec![GeneticsGenesGeneticVariants {
                        birth_event : Some(GeneticsGenesGeneticVariantsBirthEvent { event
                        : Some("minecraft:panda_lazy".to_string()), filters : None,
                        target : Some("self".to_string()) }), both_allele : None,
                        either_allele : None, hidden_allele : None, main_allele :
                        Some(crate ::types::RangeOrVal::Fixed(0f32)), mutation_rate :
                        None }, GeneticsGenesGeneticVariants { birth_event :
                        Some(GeneticsGenesGeneticVariantsBirthEvent { event :
                        Some("minecraft:panda_worried".to_string()), filters : None,
                        target : Some("self".to_string()) }), both_allele : None,
                        either_allele : None, hidden_allele : None, main_allele :
                        Some(crate ::types::RangeOrVal::Fixed(1f32)), mutation_rate :
                        None }, GeneticsGenesGeneticVariants { birth_event :
                        Some(GeneticsGenesGeneticVariantsBirthEvent { event :
                        Some("minecraft:panda_playful".to_string()), filters : None,
                        target : Some("self".to_string()) }), both_allele : None,
                        either_allele : None, hidden_allele : None, main_allele :
                        Some(crate ::types::RangeOrVal::Fixed(2f32)), mutation_rate :
                        None }, GeneticsGenesGeneticVariants { birth_event :
                        Some(GeneticsGenesGeneticVariantsBirthEvent { event :
                        Some("minecraft:panda_aggressive".to_string()), filters : None,
                        target : Some("self".to_string()) }), both_allele : None,
                        either_allele : None, hidden_allele : None, main_allele :
                        Some(crate ::types::RangeOrVal::Fixed(3f32)), mutation_rate :
                        None }, GeneticsGenesGeneticVariants { birth_event :
                        Some(GeneticsGenesGeneticVariantsBirthEvent { event :
                        Some("minecraft:panda_weak".to_string()), filters : None, target
                        : Some("self".to_string()) }), both_allele : Some(crate
                        ::types::RangeOrVal::Range { min : 4f32, max : 7f32 }),
                        either_allele : None, hidden_allele : None, main_allele : None,
                        mutation_rate : None }, GeneticsGenesGeneticVariants {
                        birth_event : Some(GeneticsGenesGeneticVariantsBirthEvent { event
                        : Some("minecraft:panda_brown".to_string()), filters : None,
                        target : Some("self".to_string()) }), both_allele : Some(crate
                        ::types::RangeOrVal::Range { min : 8f32, max : 9f32 }),
                        either_allele : None, hidden_allele : None, main_allele : None,
                        mutation_rate : None }]), name : Some("panda_variant"
                        .to_string()) }
                    ],
                ),
                mutation_rate: Some(0.03125f32),
            },
            giveable: super::super::components::Giveable {
                triggers: Some(GiveableTriggers {
                    cooldown: Some(3f32),
                    items: Some(
                        vec![
                            crate ::types::BedrockValue::String("bamboo".to_string()),
                            crate ::types::BedrockValue::String("cake".to_string())
                        ],
                    ),
                    on_give: Some(GiveableTriggersOnGive {
                        event: Some("minecraft:on_calm".to_string()),
                        filters: None,
                        target: Some("self".to_string()),
                    }),
                }),
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
                inventory_size: Some(1i32),
                private: Some(true),
                restrict_to_owner: Some(false),
            },
            is_hidden_when_invisible: super::super::components::IsHiddenWhenInvisible,
            jump_static: super::super::components::JumpStatic {
                jump_power: Some(0.42f32),
            },
            movement: super::super::components::Movement {
                max: None,
                min: None,
                value: crate::types::RangeOrVal::Fixed(0.15f32),
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
                avoid_sun: Some(false),
                avoid_water: Some(true),
                blocks_to_avoid: None,
                can_breach: Some(false),
                can_break_doors: Some(false),
                can_float: Some(true),
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
            rideable: super::super::components::Rideable {
                controlling_seat: Some(0i32),
                crouching_skip_interact: Some(true),
                dismount_mode: Some("default".to_string()),
                family_types: Some(
                    vec!["baby_zombie".to_string(), "baby_husk".to_string()],
                ),
                interact_text: None,
                on_rider_enter_event: None,
                on_rider_exit_event: None,
                passenger_max_width: Some(0f32),
                pull_in_entities: Some(false),
                rider_can_interact: Some(false),
                seat_count: Some(1i32),
                seats: Some(
                    vec![
                        RideableSeats { camera_relax_distance_smoothing : None,
                        lock_rider_rotation : None, max_rider_count : None,
                        min_rider_count : None, position : None, rotate_rider_by : None,
                        third_person_camera_radius : None }
                    ],
                ),
            },
            scale: super::super::components::Scale {
                value: 1f32,
            },
            type_family: super::super::components::TypeFamily {
                family: vec!["panda".to_string()],
            },
            variant: super::super::components::Variant {
                value: 0i32,
            },
            water_movement: super::super::components::WaterMovement {
                drag_factor: Some(0.98f32),
            },
        })
        .id()
}
/// Component groups for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PandaComponentGroup {
    BabyScared,
    PandaAdult,
    PandaAggressive,
    PandaAngry,
    PandaBaby,
    PandaBrown,
    PandaLazy,
    PandaPlayful,
    PandaSneezing,
    PandaWeak,
    PandaWorried,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PandaEvent {
    AgeableGrowUp,
    BabyOnCalm,
    BecomeAngry,
    EntityBorn,
    EntitySpawned,
    OnCalm,
    OnScared,
    PandaAggressive,
    PandaBrown,
    PandaLazy,
    PandaPlayful,
    PandaWeak,
    PandaWorried,
}
