//! Generated definition for entity.
#[allow(unused_imports)]
use super::super::components::*;
use bevy_ecs::prelude::{Bundle, Commands, Entity};
/// Entity definition for `minecraft:player`
pub struct Player;
impl Player {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:player";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = false;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = false;
}
/// Component bundle for spawning a `minecraft:player`
#[derive(Bundle, Clone)]
pub struct PlayerBundle {
    pub attack: super::super::components::Attack,
    pub block_climber: super::super::components::BlockClimber,
    pub breathable: super::super::components::Breathable,
    pub can_climb: super::super::components::CanClimb,
    pub collision_box: super::super::components::CollisionBox,
    pub conditional_bandwidth_optimization:
        super::super::components::ConditionalBandwidthOptimization,
    pub environment_sensor: super::super::components::EnvironmentSensor,
    pub exhaustion_values: super::super::components::ExhaustionValues,
    pub experience_reward: super::super::components::ExperienceReward,
    pub hurt_on_condition: super::super::components::HurtOnCondition,
    pub insomnia: super::super::components::Insomnia,
    pub is_hidden_when_invisible: super::super::components::IsHiddenWhenInvisible,
    pub loot: super::super::components::Loot,
    pub movement: super::super::components::Movement,
    pub nameable: super::super::components::Nameable,
    pub physics: super::super::components::Physics,
    pub player_exhaustion: super::super::components::PlayerExhaustion,
    pub player_experience: super::super::components::PlayerExperience,
    pub player_level: super::super::components::PlayerLevel,
    pub player_saturation: super::super::components::PlayerSaturation,
    pub pushable: super::super::components::Pushable,
    pub rideable: super::super::components::Rideable,
    pub type_family: super::super::components::TypeFamily,
}
/// Spawn a new `minecraft:player` entity with default Bedrock components
pub fn spawn_player(commands: &mut Commands) -> Entity {
    commands
        .spawn(PlayerBundle {
            attack: super::super::components::Attack {
                damage: crate::types::RangeOrVal::Fixed(1f32),
                effect_duration: Some(crate::types::MolangOr::Value(0i32)),
                effect_name: None,
            },
            block_climber: super::super::components::BlockClimber,
            breathable: super::super::components::Breathable {
                breathe_blocks: None,
                breathes_air: Some(true),
                breathes_lava: Some(false),
                breathes_solids: Some(false),
                breathes_water: Some(false),
                can_dehydrate: Some(false),
                generates_bubbles: Some(false),
                inhale_time: Some(3.75f32),
                non_breathe_blocks: None,
                suffocate_time: Some(-1i32),
                total_supply: Some(15i32),
            },
            can_climb: super::super::components::CanClimb,
            collision_box: super::super::components::CollisionBox {
                height: Some(1.8f32),
                width: Some(0.6f32),
            },
            conditional_bandwidth_optimization:
                super::super::components::ConditionalBandwidthOptimization {
                    conditional_values: None,
                    default_values: None,
                },
            environment_sensor: super::super::components::EnvironmentSensor {
                triggers: Some(crate::types::BedrockValue::Object(
                    std::collections::HashMap::from([
                        (
                            "event".to_string(),
                            crate::types::BedrockValue::String(
                                "minecraft:gain_raid_omen".to_string(),
                            ),
                        ),
                        (
                            "filters".to_string(),
                            crate::types::BedrockValue::Object(std::collections::HashMap::from([
                                (
                                    "all_of".to_string(),
                                    crate::types::BedrockValue::Array(vec![
                                        crate::types::BedrockValue::Object(
                                            std::collections::HashMap::from([
                                                (
                                                    "subject".to_string(),
                                                    crate::types::BedrockValue::String(
                                                        "self".to_string(),
                                                    ),
                                                ),
                                                (
                                                    "test".to_string(),
                                                    crate::types::BedrockValue::String(
                                                        "has_mob_effect".to_string(),
                                                    ),
                                                ),
                                                (
                                                    "value".to_string(),
                                                    crate::types::BedrockValue::String(
                                                        "bad_omen".to_string(),
                                                    ),
                                                ),
                                            ]),
                                        ),
                                        crate::types::BedrockValue::Object(
                                            std::collections::HashMap::from([
                                                (
                                                    "subject".to_string(),
                                                    crate::types::BedrockValue::String(
                                                        "self".to_string(),
                                                    ),
                                                ),
                                                (
                                                    "test".to_string(),
                                                    crate::types::BedrockValue::String(
                                                        "is_in_village".to_string(),
                                                    ),
                                                ),
                                                (
                                                    "value".to_string(),
                                                    crate::types::BedrockValue::Bool(true),
                                                ),
                                            ]),
                                        ),
                                    ]),
                                ),
                            ])),
                        ),
                    ]),
                )),
            },
            exhaustion_values: super::super::components::ExhaustionValues {
                attack: Some(0.1f32),
                damage: Some(0.1f32),
                heal: Some(6f32),
                jump: Some(0.05f32),
                lunge: Some(4f32),
                mine: Some(0.005f32),
                sprint: Some(0.1f32),
                sprint_jump: Some(0.2f32),
                swim: Some(0.01f32),
                walk: Some(0f32),
            },
            experience_reward: super::super::components::ExperienceReward {
                on_bred: Some(crate::types::MolangOr::Value(0f32)),
                on_death: Some(crate::types::MolangOr::Expr(
                    "Math.Min(query.player_level * 7, 100)".to_string(),
                )),
            },
            hurt_on_condition: super::super::components::HurtOnCondition {
                damage_conditions: None,
            },
            insomnia: super::super::components::Insomnia {
                days_until_insomnia: Some(3f32),
            },
            is_hidden_when_invisible: super::super::components::IsHiddenWhenInvisible,
            loot: super::super::components::Loot {
                table: "loot_tables/empty.json".to_string(),
            },
            movement: super::super::components::Movement {
                max: None,
                min: None,
                value: crate::types::RangeOrVal::Fixed(0.1f32),
            },
            nameable: super::super::components::Nameable {
                allow_name_tag_renaming: Some(false),
                always_show: Some(true),
                default_trigger: None,
                name_actions: None,
            },
            physics: super::super::components::Physics {
                has_collision: Some(true),
                has_gravity: Some(true),
                push_towards_closest_space: Some(true),
            },
            player_exhaustion: super::super::components::PlayerExhaustion {
                max: Some(20i32),
                value: 0i32,
            },
            player_experience: super::super::components::PlayerExperience {
                max: Some(1i32),
                value: 0i32,
            },
            player_level: super::super::components::PlayerLevel {
                max: Some(24791i32),
                value: 0i32,
            },
            player_saturation: super::super::components::PlayerSaturation {
                max: Some(20i32),
                value: 5i32,
            },
            pushable: super::super::components::Pushable {
                is_pushable: Some(false),
                is_pushable_by_piston: Some(true),
            },
            rideable: super::super::components::Rideable {
                controlling_seat: Some(0i32),
                crouching_skip_interact: Some(true),
                dismount_mode: Some("default".to_string()),
                family_types: Some(vec!["parrot_tame".to_string()]),
                interact_text: None,
                on_rider_enter_event: None,
                on_rider_exit_event: None,
                passenger_max_width: Some(0f32),
                pull_in_entities: Some(true),
                rider_can_interact: Some(false),
                seat_count: Some(2i32),
                seats: Some(vec![
                    RideableSeats {
                        camera_relax_distance_smoothing: None,
                        lock_rider_rotation: Some(0f32),
                        max_rider_count: Some(0i32),
                        min_rider_count: Some(0i32),
                        position: None,
                        rotate_rider_by: None,
                        third_person_camera_radius: None,
                    },
                    RideableSeats {
                        camera_relax_distance_smoothing: None,
                        lock_rider_rotation: Some(0f32),
                        max_rider_count: Some(2i32),
                        min_rider_count: Some(1i32),
                        position: None,
                        rotate_rider_by: None,
                        third_person_camera_radius: None,
                    },
                ]),
            },
            type_family: super::super::components::TypeFamily {
                family: vec!["player".to_string()],
            },
        })
        .id()
}
/// Component groups for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PlayerComponentGroup {
    AddRaidOmen,
    ClearRaidOmenSpellEffect,
    RaidTrigger,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PlayerEvent {
    ClearAddRaidOmen,
    GainRaidOmen,
    RemoveRaidTrigger,
    TriggerRaid,
}
