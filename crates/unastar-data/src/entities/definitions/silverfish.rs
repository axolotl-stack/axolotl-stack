//! Generated definition for entity.
use super::super::components::*;
use bevy_ecs::prelude::*;
/// Entity definition for `minecraft:silverfish`
pub struct Silverfish;
impl Silverfish {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:silverfish";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:silverfish`
#[derive(Bundle, Clone)]
pub struct SilverfishBundle {
    pub behavior_float: BehaviorFloat,
    pub behavior_silverfish_merge_with_stone: BehaviorSilverfishMergeWithStone,
    pub block_climber: BlockClimber,
    pub can_climb: CanClimb,
    pub collision_box: CollisionBox,
    pub experience_reward: ExperienceReward,
    pub is_hidden_when_invisible: IsHiddenWhenInvisible,
    pub jump_static: JumpStatic,
    pub loot: Loot,
    pub movement_basic: MovementBasic,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:silverfish` entity with default Bedrock components
pub fn spawn_silverfish(commands: &mut Commands) -> Entity {
    commands
        .spawn(SilverfishBundle {
            behavior_float: BehaviorFloat {
                chance_per_tick_to_float: Some(0.8f32),
                priority: Some(1i32),
                sink_with_passengers: Some(false),
                time_under_water_to_dismount_passengers: Some(0f32),
            },
            behavior_silverfish_merge_with_stone: BehaviorSilverfishMergeWithStone {
                priority: Some(5i32),
            },
            block_climber: BlockClimber,
            can_climb: CanClimb,
            collision_box: CollisionBox {
                height: Some(0.3f32),
                width: Some(0.4f32),
            },
            experience_reward: ExperienceReward {
                on_bred: None,
                on_death: Some("query.last_hit_by_player ? 5 : 0".to_string()),
            },
            is_hidden_when_invisible: IsHiddenWhenInvisible,
            jump_static: JumpStatic {
                jump_power: Some(0.42f32),
            },
            loot: Loot {
                table: "loot_tables/entities/silverfish.json".to_string(),
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
        })
        .id()
}
/// Component groups for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SilverfishComponentGroup {
    SilverfishAngry,
    SilverfishCalm,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SilverfishEvent {
    BecomeAngry,
    EntitySpawned,
    OnCalm,
}
