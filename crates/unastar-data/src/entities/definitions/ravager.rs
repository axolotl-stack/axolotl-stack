//! Generated definition for entity.
#[allow(unused_imports)]
use super::super::components::*;
use bevy_ecs::prelude::{Bundle, Commands, Entity};
/// Entity definition for `minecraft:ravager`
pub struct Ravager;
impl Ravager {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:ravager";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:ravager`
#[derive(Bundle, Clone)]
pub struct RavagerBundle {
    pub attack: super::super::components::Attack,
    pub behavior_float: super::super::components::BehaviorFloat,
    pub break_blocks: super::super::components::BreakBlocks,
    pub breathable: super::super::components::Breathable,
    pub can_join_raid: super::super::components::CanJoinRaid,
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
    pub knockback_resistance: super::super::components::KnockbackResistance,
    pub loot: super::super::components::Loot,
    pub movement: super::super::components::Movement,
    pub movement_basic: super::super::components::MovementBasic,
    pub nameable: super::super::components::Nameable,
    pub navigation_walk: super::super::components::NavigationWalk,
    pub physics: super::super::components::Physics,
    pub pushable: super::super::components::Pushable,
    pub ravager_blocked: super::super::components::RavagerBlocked,
    pub rideable: super::super::components::Rideable,
    pub type_family: super::super::components::TypeFamily,
}
/// Spawn a new `minecraft:ravager` entity with default Bedrock components
pub fn spawn_ravager(commands: &mut Commands) -> Entity {
    commands
        .spawn(RavagerBundle {
            attack: super::super::components::Attack {
                damage: crate::types::RangeOrVal::Fixed(12f32),
                effect_duration: Some(crate::types::MolangOr::Value(0i32)),
                effect_name: None,
            },
            behavior_float: super::super::components::BehaviorFloat {
                chance_per_tick_to_float: Some(0.8f32),
                priority: Some(BehaviorFloatPriority {}),
                sink_with_passengers: Some(false),
                time_under_water_to_dismount_passengers: Some(0f32),
            },
            break_blocks: super::super::components::BreakBlocks {
                breakable_blocks: Some(vec![
                    "bamboo".to_string(),
                    "bamboo_sapling".to_string(),
                    "beetroot".to_string(),
                    "brown_mushroom".to_string(),
                    "carrots".to_string(),
                    "carved_pumpkin".to_string(),
                    "chorus_flower".to_string(),
                    "chorus_plant".to_string(),
                    "deadbush".to_string(),
                    "double_plant".to_string(),
                    "leaves".to_string(),
                    "leaves2".to_string(),
                    "lit_pumpkin".to_string(),
                    "melon_block".to_string(),
                    "melon_stem".to_string(),
                    "potatoes".to_string(),
                    "pumpkin".to_string(),
                    "pumpkin_stem".to_string(),
                    "red_flower".to_string(),
                    "red_mushroom".to_string(),
                    "crimson_fungus".to_string(),
                    "warped_fungus".to_string(),
                    "reeds".to_string(),
                    "sapling".to_string(),
                    "snow_layer".to_string(),
                    "sweet_berry_bush".to_string(),
                    "tallgrass".to_string(),
                    "turtle_egg".to_string(),
                    "vine".to_string(),
                    "waterlily".to_string(),
                    "wheat".to_string(),
                    "dandelion".to_string(),
                    "azalea".to_string(),
                    "flowering_azalea".to_string(),
                    "azalea_leaves".to_string(),
                    "azalea_leaves_flowered".to_string(),
                    "cave_vines".to_string(),
                    "cave_vines_body_with_berries".to_string(),
                    "cave_vines_head_with_berries".to_string(),
                    "small_dripleaf_block".to_string(),
                    "big_dripleaf".to_string(),
                    "spore_blossom".to_string(),
                    "hanging_roots".to_string(),
                    "mangrove_leaves".to_string(),
                    "pale_hanging_moss".to_string(),
                    "cherry_leaves".to_string(),
                    "pale_oak_leaves".to_string(),
                    "firefly_bush".to_string(),
                    "bush".to_string(),
                ]),
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
                height: Some(2.2f32),
                width: Some(1.95f32),
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
            experience_reward: super::super::components::ExperienceReward {
                on_bred: Some(crate::types::MolangOr::Value(0f32)),
                on_death: Some(crate::types::MolangOr::Expr(
                    "query.last_hit_by_player ? 20 : 0".to_string(),
                )),
            },
            follow_range: super::super::components::FollowRange {
                max: None,
                min: None,
                value: crate::types::RangeOrVal::Fixed(64f32),
            },
            health: super::super::components::Health {
                max: Some(100f32),
                min: None,
                value: crate::types::RangeOrVal::Fixed(100f32),
            },
            hurt_on_condition: super::super::components::HurtOnCondition {
                damage_conditions: None,
            },
            is_hidden_when_invisible: super::super::components::IsHiddenWhenInvisible,
            jump_static: super::super::components::JumpStatic {
                jump_power: Some(0.42f32),
            },
            knockback_resistance: super::super::components::KnockbackResistance {
                max: None,
                min: None,
                value: crate::types::RangeOrVal::Fixed(0.75f32),
            },
            loot: super::super::components::Loot {
                table: "loot_tables/entities/ravager.json".to_string(),
            },
            movement: super::super::components::Movement {
                max: None,
                min: None,
                value: crate::types::RangeOrVal::Fixed(0f32),
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
                can_sink: Some(false),
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
            ravager_blocked: super::super::components::RavagerBlocked {
                knockback_strength: Some(3f32),
                reaction_choices: Some(vec![
                    RavagerBlockedReactionChoices {
                        value: crate::types::BedrockValue::Object(std::collections::HashMap::from(
                            [
                                (
                                    "event".to_string(),
                                    crate::types::BedrockValue::String(
                                        "minecraft:become_stunned".to_string(),
                                    ),
                                ),
                                (
                                    "target".to_string(),
                                    crate::types::BedrockValue::String("self".to_string()),
                                ),
                            ],
                        )),
                        weight: Some(1i32),
                    },
                    RavagerBlockedReactionChoices {
                        value: crate::types::BedrockValue::Null,
                        weight: Some(1i32),
                    },
                ]),
            },
            rideable: super::super::components::Rideable {
                controlling_seat: Some(0i32),
                crouching_skip_interact: Some(true),
                dismount_mode: Some("default".to_string()),
                family_types: Some(vec![
                    "pillager".to_string(),
                    "vindicator".to_string(),
                    "evocation_illager".to_string(),
                ]),
                interact_text: None,
                on_rider_enter_event: None,
                on_rider_exit_event: None,
                passenger_max_width: Some(0f32),
                pull_in_entities: Some(false),
                rider_can_interact: Some(false),
                seat_count: Some(1i32),
                seats: Some(vec![RideableSeats {
                    camera_relax_distance_smoothing: None,
                    lock_rider_rotation: None,
                    max_rider_count: None,
                    min_rider_count: None,
                    position: None,
                    rotate_rider_by: None,
                    third_person_camera_radius: None,
                }]),
            },
            type_family: super::super::components::TypeFamily {
                family: vec![
                    "monster".to_string(),
                    "ravager".to_string(),
                    "mob".to_string(),
                ],
            },
        })
        .id()
}
/// Component groups for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RavagerComponentGroup {
    Celebrate,
    EvokerRiderForRaid,
    Hostile,
    PillagerCaptainRider,
    PillagerRider,
    PillagerRiderForRaid,
    RaidConfiguration,
    RaidPersistence,
    VindicatorCaptainRider,
    VindicatorRider,
    Roaring,
    Stunned,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RavagerEvent {
    BecomeStunned,
    EndRoar,
    EntitySpawned,
    RaidExpired,
    SpawnForRaid,
    SpawnForRaidWithEvokerRider,
    SpawnForRaidWithPillagerRider,
    SpawnWithPillagerCaptainRider,
    SpawnWithPillagerRider,
    SpawnWithVindicatorCaptainRider,
    SpawnWithVindicatorRider,
    StartCelebrating,
    StartRoar,
    StopCelebrating,
}
