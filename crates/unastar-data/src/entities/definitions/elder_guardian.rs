//! Generated definition for entity.
use super::super::components::*;
use bevy_ecs::prelude::*;
/// Entity definition for `minecraft:elder_guardian`
pub struct ElderGuardian;
impl ElderGuardian {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:elder_guardian";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:elder_guardian`
#[derive(Bundle, Clone)]
pub struct ElderGuardianBundle {
    pub behavior_move_towards_home_restriction: BehaviorMoveTowardsHomeRestriction,
    pub behavior_random_swim: BehaviorRandomSwim,
    pub collision_box: CollisionBox,
    pub experience_reward: ExperienceReward,
    pub is_hidden_when_invisible: IsHiddenWhenInvisible,
    pub jump_static: JumpStatic,
    pub loot: Loot,
    pub movement_sway: MovementSway,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:elder_guardian` entity with default Bedrock components
pub fn spawn_elder_guardian(commands: &mut Commands) -> Entity {
    commands
        .spawn(ElderGuardianBundle {
            behavior_move_towards_home_restriction: BehaviorMoveTowardsHomeRestriction {
                priority: Some(5i32),
                speed_multiplier: Some(1f32),
            },
            behavior_random_swim: BehaviorRandomSwim {
                avoid_surface: Some(false),
                interval: Some(120i32),
                priority: Some(7i32),
                speed_multiplier: Some(0.5f32),
                xz_dist: Some(10i32),
                y_dist: Some(7i32),
            },
            collision_box: CollisionBox {
                height: Some(1.99f32),
                width: Some(1.99f32),
            },
            experience_reward: ExperienceReward {
                on_bred: None,
                on_death: Some("query.last_hit_by_player ? 10 : 0".to_string()),
            },
            is_hidden_when_invisible: IsHiddenWhenInvisible,
            jump_static: JumpStatic {
                jump_power: Some(0.42f32),
            },
            loot: Loot {
                table: "loot_tables/entities/elder_guardian.json".to_string(),
            },
            movement_sway: MovementSway {
                max_turn: Some(30f32),
                sway_amplitude: Some(0.05f32),
                sway_frequency: Some(0.5f32),
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
