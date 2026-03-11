//! Generated definition for entity.
use super::super::components::*;
use bevy_ecs::prelude::*;
/// Entity definition for `minecraft:ghast`
pub struct Ghast;
impl Ghast {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:ghast";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:ghast`
#[derive(Bundle, Clone)]
pub struct GhastBundle {
    pub behavior_float: BehaviorFloat,
    pub behavior_ranged_attack: BehaviorRangedAttack,
    pub cannot_be_attacked: CannotBeAttacked,
    pub collision_box: CollisionBox,
    pub experience_reward: ExperienceReward,
    pub fire_immune: FireImmune,
    pub is_hidden_when_invisible: IsHiddenWhenInvisible,
    pub jump_static: JumpStatic,
    pub loot: Loot,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:ghast` entity with default Bedrock components
pub fn spawn_ghast(commands: &mut Commands) -> Entity {
    commands
        .spawn(GhastBundle {
            behavior_float: BehaviorFloat {
                chance_per_tick_to_float: Some(0.8f32),
                priority: Some(0i32),
                sink_with_passengers: Some(false),
                time_under_water_to_dismount_passengers: Some(0f32),
            },
            behavior_ranged_attack: BehaviorRangedAttack {
                attack_interval: Some(0f32),
                attack_interval_max: Some(0f32),
                attack_interval_min: Some(0f32),
                attack_radius: Some(64f32),
                attack_radius_min: Some(0f32),
                burst_interval: Some(0f32),
                burst_shots: Some(1i32),
                charge_charged_trigger: Some(1f32),
                charge_shoot_trigger: Some(2f32),
                priority: Some(1i32),
                ranged_fov: Some(90f32),
                set_persistent: Some(false),
                speed_multiplier: Some(1f32),
                swing: Some(false),
                target_in_sight_time: Some(1f32),
                x_max_rotation: Some(30f32),
                y_max_head_rotation: Some(30f32),
            },
            cannot_be_attacked: CannotBeAttacked,
            collision_box: CollisionBox {
                height: Some(4f32),
                width: Some(4.02f32),
            },
            experience_reward: ExperienceReward {
                on_bred: None,
                on_death: Some(
                    "query.last_hit_by_player ? 5 + (query.equipment_count * Math.Random(1,3)) : 0"
                        .to_string(),
                ),
            },
            fire_immune: FireImmune,
            is_hidden_when_invisible: IsHiddenWhenInvisible,
            jump_static: JumpStatic {
                jump_power: Some(0.42f32),
            },
            loot: Loot {
                table: "loot_tables/entities/ghast.json".to_string(),
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
