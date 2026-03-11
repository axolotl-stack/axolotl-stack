//! Generated definition for entity.
use super::super::components::*;
use bevy_ecs::prelude::*;
/// Entity definition for `minecraft:shulker`
pub struct Shulker;
impl Shulker {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:shulker";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:shulker`
#[derive(Bundle, Clone)]
pub struct ShulkerBundle {
    pub behavior_ranged_attack: BehaviorRangedAttack,
    pub collision_box: CollisionBox,
    pub experience_reward: ExperienceReward,
    pub fire_immune: FireImmune,
    pub is_collidable: IsCollidable,
    pub is_hidden_when_invisible: IsHiddenWhenInvisible,
    pub loot: Loot,
    pub movement_basic: MovementBasic,
    pub physics: Physics,
    pub pushable: Pushable,
    pub renders_when_invisible: RendersWhenInvisible,
}
/// Spawn a new `minecraft:shulker` entity with default Bedrock components
pub fn spawn_shulker(commands: &mut Commands) -> Entity {
    commands
        .spawn(ShulkerBundle {
            behavior_ranged_attack: BehaviorRangedAttack {
                attack_interval: Some(0f32),
                attack_interval_max: Some(3f32),
                attack_interval_min: Some(1f32),
                attack_radius: Some(15f32),
                attack_radius_min: Some(0f32),
                burst_interval: Some(0f32),
                burst_shots: Some(1i32),
                charge_charged_trigger: Some(0f32),
                charge_shoot_trigger: Some(0f32),
                priority: None,
                ranged_fov: Some(90f32),
                set_persistent: Some(false),
                speed_multiplier: Some(1f32),
                swing: Some(false),
                target_in_sight_time: Some(1f32),
                x_max_rotation: Some(30f32),
                y_max_head_rotation: Some(30f32),
            },
            collision_box: CollisionBox {
                height: Some(1.8f32),
                width: Some(0.6f32),
            },
            experience_reward: ExperienceReward {
                on_bred: None,
                on_death: Some("query.last_hit_by_player ? 5: 0".to_string()),
            },
            fire_immune: FireImmune,
            is_collidable: IsCollidable,
            is_hidden_when_invisible: IsHiddenWhenInvisible,
            loot: Loot {
                table: "loot_tables/entities/shulker.json".to_string(),
            },
            movement_basic: MovementBasic {
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
            renders_when_invisible: RendersWhenInvisible,
        })
        .id()
}
/// Component groups for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ShulkerComponentGroup {
    ShulkerBlack,
    ShulkerBlue,
    ShulkerBrown,
    ShulkerCyan,
    ShulkerGray,
    ShulkerGreen,
    ShulkerLightBlue,
    ShulkerLime,
    ShulkerMagenta,
    ShulkerOrange,
    ShulkerPink,
    ShulkerPurple,
    ShulkerRed,
    ShulkerSilver,
    ShulkerUndyed,
    ShulkerWhite,
    ShulkerYellow,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ShulkerEvent {
    EntitySpawned,
    TurnBlack,
    TurnBlue,
    TurnBrown,
    TurnCyan,
    TurnGray,
    TurnGreen,
    TurnLightBlue,
    TurnLime,
    TurnMagenta,
    TurnOrange,
    TurnPink,
    TurnPurple,
    TurnRed,
    TurnSilver,
    TurnWhite,
    TurnYellow,
}
