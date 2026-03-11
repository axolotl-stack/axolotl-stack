//! Generated definition for entity.
#[allow(unused_imports)]
use super::super::components::*;
use bevy_ecs::prelude::{Bundle, Commands, Entity};
/// Entity definition for `minecraft:glow_squid`
pub struct GlowSquid;
impl GlowSquid {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:glow_squid";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:glow_squid`
#[derive(Bundle, Clone)]
pub struct GlowSquidBundle {
    pub balloonable: super::super::components::Balloonable,
    pub behavior_squid_dive: super::super::components::BehaviorSquidDive,
    pub behavior_squid_flee: super::super::components::BehaviorSquidFlee,
    pub behavior_squid_idle: super::super::components::BehaviorSquidIdle,
    pub behavior_squid_move_away_from_ground:
        super::super::components::BehaviorSquidMoveAwayFromGround,
    pub behavior_squid_out_of_water: super::super::components::BehaviorSquidOutOfWater,
    pub breathable: super::super::components::Breathable,
    pub can_climb: super::super::components::CanClimb,
    pub collision_box: super::super::components::CollisionBox,
    pub despawn: super::super::components::Despawn,
    pub experience_reward: super::super::components::ExperienceReward,
    pub health: super::super::components::Health,
    pub hurt_on_condition: super::super::components::HurtOnCondition,
    pub is_hidden_when_invisible: super::super::components::IsHiddenWhenInvisible,
    pub jump_static: super::super::components::JumpStatic,
    pub leashable: super::super::components::Leashable,
    pub loot: super::super::components::Loot,
    pub movement: super::super::components::Movement,
    pub movement_basic: super::super::components::MovementBasic,
    pub nameable: super::super::components::Nameable,
    pub navigation_walk: super::super::components::NavigationWalk,
    pub physics: super::super::components::Physics,
    pub pushable: super::super::components::Pushable,
    pub type_family: super::super::components::TypeFamily,
}
/// Spawn a new `minecraft:glow_squid` entity with default Bedrock components
pub fn spawn_glow_squid(commands: &mut Commands) -> Entity {
    commands
        .spawn(GlowSquidBundle {
            balloonable: super::super::components::Balloonable {
                mass: Some(0.5f32),
                max_distance: None,
                on_balloon: None,
                on_unballoon: None,
                soft_distance: None,
            },
            behavior_squid_dive: super::super::components::BehaviorSquidDive {
                priority: Some(BehaviorSquidDivePriority {}),
            },
            behavior_squid_flee: super::super::components::BehaviorSquidFlee {
                priority: Some(BehaviorSquidFleePriority {}),
            },
            behavior_squid_idle: super::super::components::BehaviorSquidIdle {
                priority: Some(BehaviorSquidIdlePriority {}),
            },
            behavior_squid_move_away_from_ground:
                super::super::components::BehaviorSquidMoveAwayFromGround {
                    priority: Some(BehaviorSquidMoveAwayFromGroundPriority {}),
                },
            behavior_squid_out_of_water: super::super::components::BehaviorSquidOutOfWater {
                priority: Some(BehaviorSquidOutOfWaterPriority {}),
            },
            breathable: super::super::components::Breathable {
                breathe_blocks: None,
                breathes_air: Some(false),
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
            can_climb: super::super::components::CanClimb,
            collision_box: super::super::components::CollisionBox {
                height: Some(0.8f32),
                width: Some(0.8f32),
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
                    "!query.is_baby && query.last_hit_by_player ? Math.Random(1,3) : 0".to_string(),
                )),
            },
            health: super::super::components::Health {
                max: Some(10f32),
                min: None,
                value: crate::types::RangeOrVal::Fixed(10f32),
            },
            hurt_on_condition: super::super::components::HurtOnCondition {
                damage_conditions: None,
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
            loot: super::super::components::Loot {
                table: "loot_tables/entities/glow_squid.json".to_string(),
            },
            movement: super::super::components::Movement {
                max: None,
                min: None,
                value: crate::types::RangeOrVal::Fixed(0.2f32),
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
            type_family: super::super::components::TypeFamily {
                family: vec![
                    "aquatic".to_string(),
                    "squid".to_string(),
                    "mob".to_string(),
                ],
            },
        })
        .id()
}
/// Component groups for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GlowSquidComponentGroup {
    SquidAdult,
    SquidBaby,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GlowSquidEvent {
    EntitySpawned,
}
