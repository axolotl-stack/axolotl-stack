//! Generated definition for entity.
use super::super::components::*;
use bevy_ecs::prelude::*;
/// Entity definition for `minecraft:slime`
pub struct Slime;
impl Slime {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:slime";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:slime`
#[derive(Bundle, Clone)]
pub struct SlimeBundle {
    pub can_climb: CanClimb,
    pub collision_box: CollisionBox,
    pub experience_reward: ExperienceReward,
    pub is_hidden_when_invisible: IsHiddenWhenInvisible,
    pub jump_static: JumpStatic,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:slime` entity with default Bedrock components
pub fn spawn_slime(commands: &mut Commands) -> Entity {
    commands
        .spawn(SlimeBundle {
            can_climb: CanClimb,
            collision_box: CollisionBox {
                height: Some(2.08f32),
                width: Some(2.08f32),
            },
            experience_reward: ExperienceReward {
                on_bred: None,
                on_death: Some("query.last_hit_by_player ? query.variant : 0".to_string()),
            },
            is_hidden_when_invisible: IsHiddenWhenInvisible,
            jump_static: JumpStatic {
                jump_power: Some(0.42f32),
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
pub enum SlimeComponentGroup {
    SlimeAggressive,
    SlimeCalm,
    SlimeLarge,
    SlimeMedium,
    SlimeSmall,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SlimeEvent {
    BecomeAggressive,
    BecomeCalm,
    EntitySpawned,
    SpawnLarge,
    SpawnMedium,
    SpawnSmall,
}
