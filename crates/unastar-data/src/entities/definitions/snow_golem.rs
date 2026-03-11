//! Generated definition for entity.
use super::super::components::*;
use bevy_ecs::prelude::*;
/// Entity definition for `minecraft:snow_golem`
pub struct SnowGolem;
impl SnowGolem {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:snow_golem";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:snow_golem`
#[derive(Bundle, Clone)]
pub struct SnowGolemBundle {
    pub balloonable: Balloonable,
    pub behavior_random_stroll: BehaviorRandomStroll,
    pub behavior_ranged_attack: BehaviorRangedAttack,
    pub can_climb: CanClimb,
    pub collision_box: CollisionBox,
    pub is_hidden_when_invisible: IsHiddenWhenInvisible,
    pub jump_static: JumpStatic,
    pub leashable_to: LeashableTo,
    pub loot: Loot,
    pub movement_basic: MovementBasic,
    pub persistent: Persistent,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:snow_golem` entity with default Bedrock components
pub fn spawn_snow_golem(commands: &mut Commands) -> Entity {
    commands
        .spawn(SnowGolemBundle {
            balloonable: Balloonable {
                mass: None,
                max_distance: None,
                on_balloon: None,
                on_unballoon: None,
                soft_distance: None,
            },
            behavior_random_stroll: BehaviorRandomStroll {
                interval: Some(120i32),
                priority: Some(2i32),
                speed_multiplier: Some(1f32),
                xz_dist: Some(10i32),
                y_dist: Some(7i32),
            },
            behavior_ranged_attack: BehaviorRangedAttack {
                attack_interval: Some(1f32),
                attack_interval_max: Some(0f32),
                attack_interval_min: Some(0f32),
                attack_radius: Some(10f32),
                attack_radius_min: Some(0f32),
                burst_interval: Some(0f32),
                burst_shots: Some(1i32),
                charge_charged_trigger: Some(0f32),
                charge_shoot_trigger: Some(0f32),
                priority: Some(1i32),
                ranged_fov: Some(90f32),
                set_persistent: Some(false),
                speed_multiplier: Some(1.25f32),
                swing: Some(false),
                target_in_sight_time: Some(1f32),
                x_max_rotation: Some(30f32),
                y_max_head_rotation: Some(30f32),
            },
            can_climb: CanClimb,
            collision_box: CollisionBox {
                height: Some(1.8f32),
                width: Some(0.4f32),
            },
            is_hidden_when_invisible: IsHiddenWhenInvisible,
            jump_static: JumpStatic {
                jump_power: Some(0.42f32),
            },
            leashable_to: LeashableTo {
                can_retrieve_from: Some(false),
            },
            loot: Loot {
                table: "loot_tables/entities/snowman.json".to_string(),
            },
            movement_basic: MovementBasic {
                max_turn: Some(30f32),
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
/// Component groups for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SnowGolemComponentGroup {
    SnowmanSheared,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SnowGolemEvent {
    OnSheared,
}
