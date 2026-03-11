//! Generated definition for entity.
use super::super::components::*;
use bevy_ecs::prelude::*;
/// Entity definition for `minecraft:frog`
pub struct Frog;
impl Frog {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:frog";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:frog`
#[derive(Bundle, Clone)]
pub struct FrogBundle {
    pub behavior_breed: BehaviorBreed,
    pub behavior_eat_mob: BehaviorEatMob,
    pub behavior_move_to_land: BehaviorMoveToLand,
    pub behavior_random_stroll: BehaviorRandomStroll,
    pub collision_box: CollisionBox,
    pub experience_reward: ExperienceReward,
    pub is_hidden_when_invisible: IsHiddenWhenInvisible,
    pub jump_static: JumpStatic,
    pub leashable_to: LeashableTo,
    pub movement_amphibious: MovementAmphibious,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:frog` entity with default Bedrock components
pub fn spawn_frog(commands: &mut Commands) -> Entity {
    commands
        .spawn(FrogBundle {
            behavior_breed: BehaviorBreed {
                priority: Some(4i32),
                speed_multiplier: Some(1f32),
            },
            behavior_eat_mob: BehaviorEatMob {
                eat_animation_time: Some(1f32),
                eat_mob_sound: Some("".to_string()),
                loot_table: Some("".to_string()),
                priority: Some(0i32),
                pull_in_force: Some(1f32),
                reach_mob_distance: Some(1f32),
                run_speed: Some(1f32),
            },
            behavior_move_to_land: BehaviorMoveToLand {
                goal_radius: Some(2f32),
                priority: Some(6i32),
                search_count: Some(80i32),
                search_height: Some(8i32),
                search_range: Some(30i32),
                speed_multiplier: Some(1f32),
            },
            behavior_random_stroll: BehaviorRandomStroll {
                interval: Some(120i32),
                priority: Some(11i32),
                speed_multiplier: Some(1f32),
                xz_dist: Some(10i32),
                y_dist: Some(7i32),
            },
            collision_box: CollisionBox {
                height: Some(0.55f32),
                width: Some(0.5f32),
            },
            experience_reward: ExperienceReward {
                on_bred: Some("Math.Random(1,7)".to_string()),
                on_death: Some("query.last_hit_by_player ? Math.Random(1,3) : 0".to_string()),
            },
            is_hidden_when_invisible: IsHiddenWhenInvisible,
            jump_static: JumpStatic {
                jump_power: Some(0.42f32),
            },
            leashable_to: LeashableTo {
                can_retrieve_from: Some(false),
            },
            movement_amphibious: MovementAmphibious {
                max_turn: Some(30f32),
            },
            physics: Physics {
                has_collision: Some(true),
                has_gravity: Some(true),
                push_towards_closest_space: Some(false),
            },
            pushable: Pushable {
                is_pushable: Some(true),
                is_pushable_by_piston: Some(true),
            },
        })
        .id()
}
/// Component groups for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FrogComponentGroup {
    ColdFrog,
    Pregnant,
    TemperateFrog,
    WarmFrog,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FrogEvent {
    BecomePregnant,
    LaidEgg,
    EntitySpawned,
    EntityTransformed,
    SpawnCold,
    SpawnTemperate,
    SpawnWarm,
}
