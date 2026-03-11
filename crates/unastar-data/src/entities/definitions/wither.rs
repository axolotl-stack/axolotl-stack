//! Generated definition for entity.
use super::super::components::*;
use bevy_ecs::prelude::*;
/// Entity definition for `minecraft:wither`
pub struct Wither;
impl Wither {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:wither";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:wither`
#[derive(Bundle, Clone)]
pub struct WitherBundle {
    pub behavior_float: BehaviorFloat,
    pub behavior_random_stroll: BehaviorRandomStroll,
    pub behavior_wither_random_attack_pos_goal: BehaviorWitherRandomAttackPosGoal,
    pub boss: Boss,
    pub can_climb: CanClimb,
    pub collision_box: CollisionBox,
    pub experience_reward: ExperienceReward,
    pub fire_immune: FireImmune,
    pub is_hidden_when_invisible: IsHiddenWhenInvisible,
    pub jump_static: JumpStatic,
    pub loot: Loot,
    pub movement_basic: MovementBasic,
    pub persistent: Persistent,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:wither` entity with default Bedrock components
pub fn spawn_wither(commands: &mut Commands) -> Entity {
    commands
        .spawn(WitherBundle {
            behavior_float: BehaviorFloat {
                chance_per_tick_to_float: Some(0.8f32),
                priority: Some(1i32),
                sink_with_passengers: Some(false),
                time_under_water_to_dismount_passengers: Some(0f32),
            },
            behavior_random_stroll: BehaviorRandomStroll {
                interval: Some(120i32),
                priority: Some(5i32),
                speed_multiplier: Some(1f32),
                xz_dist: Some(10i32),
                y_dist: Some(7i32),
            },
            behavior_wither_random_attack_pos_goal: BehaviorWitherRandomAttackPosGoal {
                priority: Some(3i32),
            },
            boss: Boss {
                hud_range: Some(55i32),
                name: Some("55".to_string()),
                should_darken_sky: Some(true),
            },
            can_climb: CanClimb,
            collision_box: CollisionBox {
                height: Some(3f32),
                width: Some(1f32),
            },
            experience_reward: ExperienceReward {
                on_bred: None,
                on_death: Some("50".to_string()),
            },
            fire_immune: FireImmune,
            is_hidden_when_invisible: IsHiddenWhenInvisible,
            jump_static: JumpStatic {
                jump_power: Some(0.42f32),
            },
            loot: Loot {
                table: "loot_tables/entities/wither_boss.json".to_string(),
            },
            movement_basic: MovementBasic {
                max_turn: Some(180f32),
            },
            persistent: Persistent,
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
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WitherEvent {
    EntitySpawned,
}
